//! Off-thread Gallery/Scene hover-preview rendering.
//!
//! A cold preview can compile shaders, initialise a Point10 scene and allocate
//! its targets. That work used to run on the window thread after the output
//! frame was presented, so the first hover of a cell could still miss a frame
//! even though its read-back was asynchronous. The worker owns the isolated
//! preview renderers, renders with cloned device/queue handles, and returns
//! pixels over a channel; the window loop only starts a job and uploads the
//! result.
//!
//! The worker is intentionally single-threaded: previews are throttled to one
//! at a time, and the newest hover replaces any queued request. It shares the
//! main device/queue so it does not create a second GPU context; its draws are
//! thumbnail-sized, and the expensive cold compilation/pipeline creation is
//! what moves off the output thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use serde_json::Value;

use modv_render::wgpu;

use crate::compositor::FrameInfo;
use crate::module_preview::{ModulePreviews, PendingPreview};
use crate::module_registry::ModuleRegistry;
use crate::scene_preview::ScenePreviews;

/// One preview request handed to the worker.
pub struct PreviewJob {
    /// The texture cache key (`module:<name>` or `scene:<id>:<row>`).
    pub key: String,
    pub kind: PreviewJobKind,
}

pub enum PreviewJobKind {
    Module {
        registry: Arc<ModuleRegistry>,
        module_name: String,
        width: u32,
        height: u32,
        frame: FrameInfo,
        /// The live output to composite a hover filter over, when requested.
        base: Option<wgpu::TextureView>,
    },
    Scene {
        registry: Arc<ModuleRegistry>,
        scene: Value,
        row_names: Vec<String>,
        scene_id: String,
        row_index: usize,
        width: u32,
        height: u32,
        revision: u64,
        frame: FrameInfo,
    },
}

/// A finished preview, successful or not.
pub struct PreviewResult {
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub result: Result<Vec<u8>, String>,
}

enum Message {
    // Boxed: a Scene job carries the Scene document/prop snapshot and is much
    // larger than the other variants.
    Job(Box<PreviewJob>),
    SetMediaRoot(PathBuf),
    Shutdown,
}

/// The window thread's handle to the preview renderer thread.
pub struct PreviewWorker {
    jobs: Sender<Message>,
    results: Receiver<PreviewResult>,
    thread: Option<JoinHandle<()>>,
}

impl PreviewWorker {
    /// Start the worker with cloned handles from the main engine.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Result<Self, String> {
        let (jobs, job_rx) = channel();
        let (result_tx, results) = channel();
        let thread = std::thread::Builder::new()
            .name("modv-preview".to_string())
            .spawn(move || run(&device, &queue, &job_rx, &result_tx))
            .map_err(|error| format!("could not spawn the preview worker: {error}"))?;
        Ok(Self {
            jobs,
            results,
            thread: Some(thread),
        })
    }

    /// Point the isolated image-input cache at the project media tree.
    pub fn set_media_root(&self, root: impl Into<PathBuf>) {
        let _ = self.jobs.send(Message::SetMediaRoot(root.into()));
    }

    /// Queue one job. The worker renders jobs in order; the window loop keeps at
    /// most one outstanding by checking [`Self::try_result`].
    pub fn submit(&self, job: PreviewJob) -> bool {
        self.jobs.send(Message::Job(Box::new(job))).is_ok()
    }

    /// Non-blocking result drain for the window loop.
    pub fn try_result(&self) -> Option<PreviewResult> {
        self.results.try_recv().ok()
    }
}

impl Drop for PreviewWorker {
    fn drop(&mut self) {
        let _ = self.jobs.send(Message::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    jobs: &Receiver<Message>,
    results: &Sender<PreviewResult>,
) {
    let mut modules: Option<ModulePreviews> = None;
    let mut scenes: Option<ScenePreviews> = None;
    let mut scene_registry: Option<Arc<ModuleRegistry>> = None;
    let mut media_root: Option<PathBuf> = None;

    while let Ok(message) = jobs.recv() {
        match message {
            Message::Shutdown => break,
            Message::SetMediaRoot(root) => {
                if let Some(previews) = &mut modules {
                    previews.set_media_root(root.clone());
                }
                media_root = Some(root);
            }
            Message::Job(job) => {
                let PreviewJob { key, kind } = *job;
                let (width, height, result) = match kind {
                    PreviewJobKind::Module {
                        registry,
                        module_name,
                        width,
                        height,
                        frame,
                        base,
                    } => {
                        let previews = modules.get_or_insert_with(|| {
                            let mut previews = ModulePreviews::new(device, queue);
                            if let Some(root) = &media_root {
                                previews.set_media_root(root.clone());
                            }
                            previews
                        });
                        let result = previews
                            .render_pending(
                                registry.as_ref(),
                                &module_name,
                                width,
                                height,
                                &frame,
                                base.as_ref(),
                            )
                            .map_err(|error| error.to_string())
                            .and_then(wait_for_preview);
                        (width, height, result)
                    }
                    PreviewJobKind::Scene {
                        registry,
                        scene,
                        row_names,
                        scene_id,
                        row_index,
                        width,
                        height,
                        revision,
                        frame,
                    } => {
                        // A rescan builds a new registry; the isolated
                        // compositor must not keep rendering the old one.
                        if scene_registry
                            .as_ref()
                            .is_none_or(|current| !Arc::ptr_eq(current, &registry))
                        {
                            scenes = Some(ScenePreviews::new(device, queue, Arc::clone(&registry)));
                            scene_registry = Some(registry);
                        }
                        let previews = scenes.as_mut().expect("scene previews initialized");
                        let result = previews
                            .render_pending(
                                &scene, &row_names, &scene_id, row_index, width, height, revision,
                                &frame,
                            )
                            .map_err(|error| error.to_string())
                            .and_then(wait_for_preview);
                        (width, height, result)
                    }
                };
                let _ = results.send(PreviewResult {
                    key,
                    width,
                    height,
                    result,
                });
            }
        }
    }
}

/// Poll a preview's map callback without a blocking `Device::poll(wait)`.
///
/// The worker shares the main device/queue; a blocking wait here would hold
/// the device while the output frame is trying to submit/present. Polling with
/// a short sleep keeps the main thread's path clear.
fn wait_for_preview(mut pending: PendingPreview) -> Result<Vec<u8>, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match pending.try_finish() {
            Ok(Some(pixels)) => return Ok(pixels),
            Ok(None) => {}
            Err(error) => return Err(error.to_string()),
        }
        if std::time::Instant::now() >= deadline {
            return Err("preview read-back timed out".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module_registry::ModuleRegistry;
    use std::time::{Duration, Instant};

    #[test]
    fn a_module_preview_renders_off_thread() {
        let Some(engine) = crate::gpu_test::gpu_engine() else {
            return;
        };
        let worker = PreviewWorker::new(engine.device.clone(), engine.queue.clone())
            .expect("preview worker starts");
        let registry = Arc::new(ModuleRegistry::builtin());
        assert!(worker.submit(PreviewJob {
            key: "module:Bands".to_string(),
            kind: PreviewJobKind::Module {
                registry,
                module_name: "Bands".to_string(),
                width: 32,
                height: 18,
                frame: FrameInfo::default(),
                base: None,
            },
        }));

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = worker.try_result() {
                assert_eq!(result.key, "module:Bands");
                let pixels = result.result.expect("the preview renders");
                assert_eq!(pixels.len(), 32 * 18 * 4);
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the preview worker never returned a frame"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
