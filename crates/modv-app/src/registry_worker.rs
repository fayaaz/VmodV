//! Background module-registry rebuilds.
//!
//! A user-ISF/MilkDrop change reparses the vendored corpus plus the user
//! folders. That is filesystem + parser work (hundreds of shaders) and used to
//! run inside the window loop, causing a visible stall whenever a `.fs`/`.mlk`
//! file changed. The builder thread owns only the scan; the window loop swaps
//! the finished `Arc<ModuleRegistry>` into the store and compositor.
//!
//! Requests are generation-stamped. A newer rescan supersedes an older one, so
//! saving several shaders in quick succession cannot apply a stale registry out
//! of order.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use crate::module_registry::ModuleRegistry;

/// The directories one rebuild should scan.
#[derive(Clone, Debug)]
pub struct RegistrySpec {
    pub isf_dir: Option<PathBuf>,
    pub extra_isf_dirs: Vec<PathBuf>,
    pub milkdrop_dirs: Vec<PathBuf>,
    pub user_module_dirs: Vec<PathBuf>,
}

/// One in-flight background rebuild, coalescing newer requests.
pub struct RegistryRescan {
    generation: u64,
    receiver: Option<Receiver<(u64, Arc<ModuleRegistry>)>>,
}

impl Default for RegistryRescan {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryRescan {
    pub fn new() -> Self {
        Self {
            generation: 0,
            receiver: None,
        }
    }

    /// Start scanning `spec`. The previous in-flight scan (if any) is abandoned;
    /// its result is dropped because it is older than this generation.
    pub fn request(&mut self, spec: RegistrySpec) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let (sender, receiver) = channel();
        let spawned = std::thread::Builder::new()
            .name("modv-registry".to_string())
            .spawn(move || {
                let registry = crate::vmodv::build_module_registry_with_user_modules(
                    spec.isf_dir.as_deref(),
                    &spec.extra_isf_dirs,
                    &spec.milkdrop_dirs,
                    &spec.user_module_dirs,
                );
                let _ = sender.send((generation, registry));
            });
        match spawned {
            Ok(_) => self.receiver = Some(receiver),
            Err(error) => {
                eprintln!("ui: WARN could not start the module-registry rescan: {error}");
                self.receiver = None;
            }
        }
    }

    /// The newest completed rebuild, if one is ready.
    pub fn try_result(&mut self) -> Option<Arc<ModuleRegistry>> {
        let receiver = self.receiver.take()?;
        match receiver.try_recv() {
            Ok((generation, registry)) => (generation == self.generation).then_some(registry),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.receiver = Some(receiver);
                None
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                eprintln!("ui: WARN the module-registry rescan thread stopped unexpectedly");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A tiny user-ISF folder the background scan can register.
    fn inline_isf_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modv-registry-worker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("inline isf dir");
        std::fs::write(
            dir.join("Worker Probe.fs"),
            r#"/*{ "CREDIT": "by test", "ISFVSN": "2" }*/
            void main() { gl_FragColor = vec4(1.0); }
            "#,
        )
        .expect("inline shader written");
        dir
    }

    #[test]
    fn a_background_rescan_returns_the_new_registry() {
        let dir = inline_isf_dir();
        let mut rescan = RegistryRescan::new();
        rescan.request(RegistrySpec {
            isf_dir: None,
            extra_isf_dirs: vec![dir.clone()],
            milkdrop_dirs: Vec::new(),
            user_module_dirs: Vec::new(),
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(registry) = rescan.try_result() {
                assert_eq!(
                    registry.kind("Worker Probe"),
                    Some(crate::module_registry::ModuleKind::Isf)
                );
                std::fs::remove_dir_all(&dir).ok();
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the background registry rescan never returned"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
