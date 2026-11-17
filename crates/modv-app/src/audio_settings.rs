//! The native shell's persisted audio-source choice and live-switch policy.
//!
//! Electron has an Audio/Video settings panel; the native shell only had the
//! startup `--audio-source`/`--audio-device` flags. The picker's choice is
//! stored in `<userData>/native-audio.json` so it survives a restart, while an
//! explicit CLI flag or `MODV_AUDIO_SOURCE`/`MODV_AUDIO_DEVICE` keeps winning
//! over it.
//!
//! Everything here is pure or filesystem-simple: no sound server is opened, so
//! the precedence, JSON round-trip, device-list conversion and the
//! "failed switch keeps the previous feed" decision are unit-testable on a
//! headless host.

use std::path::Path;

use modv_media::audio::capture::InputDeviceInfo;
use modv_ui::AudioDeviceView;

use crate::cli::AudioSourceSelection;

/// The persisted audio choice inside the shared userData directory.
pub const AUDIO_SETTINGS_FILENAME: &str = "native-audio.json";

/// One persisted capture choice (`pipewire`, or `input` with an optional
/// device id; `None` is the host default input).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AudioSettings {
    pub source: String,
    #[serde(default)]
    pub device: Option<String>,
}

impl AudioSettings {
    /// The settings document for a resolved selection.
    pub fn from_selection(selection: AudioSourceSelection, device: Option<String>) -> Self {
        Self {
            source: selection.as_str().to_string(),
            device,
        }
    }

    /// The selection this document names, if the native shell can capture it.
    ///
    /// `renderer` is not a native capture source, so a stale document naming it
    /// is ignored rather than silently muting the engine.
    pub fn selection(&self) -> Option<AudioSourceSelection> {
        match self.source.as_str() {
            "pipewire" => Some(AudioSourceSelection::Pipewire),
            "input" => Some(AudioSourceSelection::Input),
            _ => None,
        }
    }

    /// A one-line description for the startup log.
    pub fn describe(&self) -> String {
        match (self.selection(), self.device.as_deref()) {
            (Some(AudioSourceSelection::Pipewire), Some(device)) => {
                format!("system audio (pipewire, target {device})")
            }
            (Some(AudioSourceSelection::Pipewire), None) => "system audio (pipewire)".to_string(),
            (Some(AudioSourceSelection::Input), Some(device)) => {
                format!("input device {device}")
            }
            (Some(AudioSourceSelection::Input), None) => "default input".to_string(),
            _ => self.source.clone(),
        }
    }

    /// Read the document, treating a missing file as "nothing saved".
    pub fn read_from(path: &Path) -> Result<Option<Self>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let settings: Self = serde_json::from_str(&text)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        if settings.selection().is_none() {
            return Err(format!(
                "{} names unknown audio source {:?} (expected pipewire or input)",
                path.display(),
                settings.source
            ));
        }
        Ok(Some(settings))
    }

    /// Write the document atomically, creating the userData directory.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("could not serialise audio settings: {error}"))?;
        std::fs::write(&temporary, text)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }
}

/// The saved choice, unless an explicit CLI flag or environment variable owns
/// the capture selection.
///
/// An invalid/legacy document (for example `renderer`, which the native shell
/// cannot capture) resolves to `None` so the built-in fallback runs.
pub fn resolve(cli_explicit: bool, persisted: Option<&AudioSettings>) -> Option<&AudioSettings> {
    if cli_explicit {
        None
    } else {
        persisted.filter(|settings| settings.selection().is_some())
    }
}

/// Whether `--audio-source`/`--audio-device` (or their environment fallbacks)
/// were supplied, in which case they beat the persisted choice.
pub fn audio_cli_explicit(
    args: &[String],
    source_env: Option<&str>,
    device_env: Option<&str>,
) -> bool {
    let set = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    set(source_env)
        || set(device_env)
        || args.iter().any(|arg| {
            arg == "--audio-source"
                || arg.starts_with("--audio-source=")
                || arg == "--audio-device"
                || arg.starts_with("--audio-device=")
        })
}

/// The capture sources to try for a requested selection.
///
/// An explicit engine-owned `--audio-source` wins; the renderer default is
/// translated to the system monitor, then the default input.
pub fn default_sources(selection: AudioSourceSelection) -> Vec<AudioSourceSelection> {
    if selection.engine_owned() {
        vec![selection]
    } else {
        vec![AudioSourceSelection::Pipewire, AudioSourceSelection::Input]
    }
}

/// The ordered startup attempts: the persisted choice (unless explicitly
/// overridden), then the built-in monitor → input fallback.
///
/// A persisted device that has since been unplugged falls through to the
/// monitor/default input instead of leaving the engine silent.
pub fn initial_plan(
    cli_source: AudioSourceSelection,
    cli_device: Option<&str>,
    cli_explicit: bool,
    persisted: Option<&AudioSettings>,
) -> Vec<(AudioSourceSelection, Option<String>)> {
    let mut plan = Vec::new();

    if let Some(settings) = resolve(cli_explicit, persisted)
        && let Some(selection) = settings.selection()
    {
        plan.push((selection, settings.device.clone()));
    }

    let fallbacks: Vec<(AudioSourceSelection, Option<String>)> = if cli_explicit {
        default_sources(cli_source)
            .into_iter()
            .map(|selection| (selection, cli_device.map(str::to_string)))
            .collect()
    } else {
        default_sources(AudioSourceSelection::Renderer)
            .into_iter()
            .map(|selection| (selection, None))
            .collect()
    };

    for entry in fallbacks {
        if !plan.contains(&entry) {
            plan.push(entry);
        }
    }

    plan
}

/// Convert the cpal device list into the Audio panel's read model.
pub fn device_views(devices: &[InputDeviceInfo]) -> Vec<AudioDeviceView> {
    devices
        .iter()
        .map(|device| AudioDeviceView {
            id: device.id.clone(),
            name: device.name.clone(),
            channels: device.channels,
            default_sample_rate: device.default_sample_rate,
        })
        .collect()
}

/// What to do after opening the requested replacement feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchDecision<T> {
    /// The new feed opened: stop the old one and adopt this.
    Replace(T),
    /// Opening failed but a feed is still running: keep it and show the error.
    Keep { error: String },
    /// Opening failed and nothing is running: report the error, stay silent.
    Silent { error: String },
}

/// The live-switch policy: a failed open must never stop a working feed.
pub fn decide_switch<T>(had_active_feed: bool, opened: Result<T, String>) -> SwitchDecision<T> {
    match opened {
        Ok(feed) => SwitchDecision::Replace(feed),
        Err(error) => {
            if had_active_feed {
                SwitchDecision::Keep { error }
            } else {
                SwitchDecision::Silent { error }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(source: &str, device: Option<&str>) -> AudioSettings {
        AudioSettings {
            source: source.to_string(),
            device: device.map(str::to_string),
        }
    }

    fn device(name: &str, id: &str) -> InputDeviceInfo {
        InputDeviceInfo {
            name: name.to_string(),
            id: id.to_string(),
            channels: 2,
            sample_rates: vec![44_100, 48_000],
            default_sample_rate: 48_000,
        }
    }

    fn temp_path() -> (std::path::PathBuf, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "modv-audio-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = directory.join(AUDIO_SETTINGS_FILENAME);
        (directory, path)
    }

    #[test]
    fn explicit_cli_wins_over_persisted_and_invalid_documents_are_ignored() {
        let saved = settings("input", Some("alsa:usb"));

        assert_eq!(resolve(false, Some(&saved)), Some(&saved));
        assert_eq!(resolve(true, Some(&saved)), None);
        assert_eq!(resolve(false, None), None);

        let legacy = settings("renderer", None);
        assert_eq!(
            resolve(false, Some(&legacy)),
            None,
            "renderer is not a native capture source"
        );
    }

    #[test]
    fn the_explicit_flags_and_environment_are_detected() {
        let none: &[String] = &[];
        assert!(!audio_cli_explicit(none, None, None));
        assert!(!audio_cli_explicit(none, Some("  "), Some("")));

        let args = ["modv", "--audio-source", "pipewire"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(audio_cli_explicit(&args, None, None));

        let inline = ["modv", "--audio-device=alsa:usb"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(audio_cli_explicit(&inline, None, None));
        assert!(audio_cli_explicit(none, Some("input"), None));
        assert!(audio_cli_explicit(none, None, Some("alsa:usb")));
    }

    #[test]
    fn the_initial_plan_uses_the_persisted_choice_then_the_monitor_and_input() {
        let saved = settings("input", Some("alsa:usb"));

        assert_eq!(
            initial_plan(AudioSourceSelection::Renderer, None, false, Some(&saved)),
            vec![
                (AudioSourceSelection::Input, Some("alsa:usb".to_string())),
                (AudioSourceSelection::Pipewire, None),
                (AudioSourceSelection::Input, None),
            ],
            "a stale device still falls through to the defaults"
        );

        assert_eq!(
            initial_plan(
                AudioSourceSelection::Renderer,
                None,
                false,
                Some(&settings("pipewire", None))
            ),
            vec![
                (AudioSourceSelection::Pipewire, None),
                (AudioSourceSelection::Input, None),
            ]
        );
    }

    #[test]
    fn the_initial_plan_falls_back_to_the_monitor_then_input_on_a_fresh_install() {
        assert_eq!(
            initial_plan(AudioSourceSelection::Renderer, None, false, None),
            vec![
                (AudioSourceSelection::Pipewire, None),
                (AudioSourceSelection::Input, None),
            ]
        );
    }

    #[test]
    fn an_explicit_cli_selection_beats_the_persisted_choice() {
        let saved = settings("input", Some("alsa:usb"));

        assert_eq!(
            initial_plan(
                AudioSourceSelection::Input,
                Some("alsa:builtin"),
                true,
                Some(&saved)
            ),
            vec![(
                AudioSourceSelection::Input,
                Some("alsa:builtin".to_string())
            )],
            "the explicit source/device is the only startup attempt"
        );
        assert_eq!(
            initial_plan(AudioSourceSelection::Pipewire, None, true, Some(&saved)),
            vec![(AudioSourceSelection::Pipewire, None)]
        );
    }

    #[test]
    fn settings_round_trip_through_json() {
        let (directory, path) = temp_path();
        let saved = settings("input", Some("alsa:usb"));

        saved.write_to(&path).expect("write settings");
        assert_eq!(
            AudioSettings::read_from(&path).expect("read settings"),
            Some(saved.clone())
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the atomic write must not leave its temporary file behind"
        );

        saved.write_to(&path).expect("overwrite settings");
        assert_eq!(
            AudioSettings::read_from(&path).expect("re-read settings"),
            Some(settings("input", Some("alsa:usb")))
        );

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_missing_file_is_not_an_error_and_bad_documents_are() {
        let (directory, path) = temp_path();

        assert_eq!(AudioSettings::read_from(&path).expect("missing file"), None);

        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(&path, "{not json").expect("write broken json");
        assert!(AudioSettings::read_from(&path).is_err());

        std::fs::write(&path, r#"{"source":"renderer"}"#).expect("write legacy document");
        let error = AudioSettings::read_from(&path).expect_err("renderer is not capturable");
        assert!(error.contains("renderer"), "{error}");

        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn device_views_map_name_id_channels_and_rate_including_an_empty_host() {
        assert!(device_views(&[]).is_empty());

        let views = device_views(&[
            device("USB Microphone", "alsa:usb"),
            device("Built-in Audio", "alsa:builtin"),
        ]);

        assert_eq!(
            views,
            vec![
                AudioDeviceView {
                    id: "alsa:usb".to_string(),
                    name: "USB Microphone".to_string(),
                    channels: 2,
                    default_sample_rate: 48_000,
                },
                AudioDeviceView {
                    id: "alsa:builtin".to_string(),
                    name: "Built-in Audio".to_string(),
                    channels: 2,
                    default_sample_rate: 48_000,
                },
            ]
        );
    }

    #[test]
    fn a_failed_open_keeps_a_running_feed_and_reports_the_error() {
        assert_eq!(
            decide_switch(true, Ok("new feed")),
            SwitchDecision::Replace("new feed")
        );
        assert_eq!(
            decide_switch::<&str>(true, Err("device busy".to_string())),
            SwitchDecision::Keep {
                error: "device busy".to_string()
            },
            "the old feed must keep running when the new selection fails"
        );
        assert_eq!(
            decide_switch::<&str>(false, Err("no device".to_string())),
            SwitchDecision::Silent {
                error: "no device".to_string()
            }
        );
    }
}
