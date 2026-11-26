//! modv-engine binary — the engine process. See AGENTS.md.
//!
//! M0 spike harness plus the M1 supervision channel and the M2 store +
//! compositor: windowed rendering of the engine state, control socket,
//! snapshot/restore and the shared-memory preview ring.

#[cfg(all(test, not(feature = "web")))]
mod gpu_test;

// Platform-neutral core: the WebAssembly shell (`modv-web`) composes these
// with browser I/O; the desktop shell composes them with winit/media/control
// sockets.
pub mod compositor;
pub mod module_preview;
pub mod module_registry;
pub mod quality;
pub mod scene_transition;
pub mod texture_sources;

// Desktop-only modules. The `web` feature leaves them out so the same crate
// can be compiled for wasm32 without GStreamer, PipeWire, ALSA, MIDI, NDI,
// inotify, Unix sockets or the native window loop.
#[cfg(not(feature = "web"))]
pub mod audio;
#[cfg(not(feature = "web"))]
pub mod audio_settings;
#[cfg(not(feature = "web"))]
pub mod cli;
#[cfg(not(feature = "web"))]
pub mod control_socket;
#[cfg(not(feature = "web"))]
pub mod expressions;
#[cfg(not(feature = "web"))]
pub mod fixtures;
#[cfg(not(feature = "web"))]
pub mod headless;
#[cfg(not(feature = "web"))]
pub mod ids;
#[cfg(not(feature = "web"))]
pub mod input_links;
#[cfg(not(feature = "web"))]
pub mod logging;
#[cfg(not(feature = "web"))]
pub mod media;
#[cfg(not(feature = "web"))]
pub mod media_dialogs;
#[cfg(not(feature = "web"))]
pub mod media_import;
#[cfg(not(feature = "web"))]
pub mod media_library;
#[cfg(not(feature = "web"))]
pub mod media_probe;
#[cfg(not(feature = "web"))]
pub mod media_profiles;
#[cfg(not(feature = "web"))]
pub mod media_settings;
#[cfg(not(feature = "web"))]
pub mod media_watch;
#[cfg(not(feature = "web"))]
pub mod midi;
#[cfg(not(feature = "web"))]
pub mod module_docs;
#[cfg(not(feature = "web"))]
pub mod ndi_readback;
#[cfg(not(feature = "web"))]
pub mod ndi_runtime;
#[cfg(not(feature = "web"))]
pub mod pacing;
#[cfg(not(feature = "web"))]
pub mod performance;
#[cfg(not(feature = "web"))]
pub mod plugin_runtime;
#[cfg(not(feature = "web"))]
pub mod preview_worker;
#[cfg(not(feature = "web"))]
pub mod registry_worker;
#[cfg(not(feature = "web"))]
pub mod render_settings;
#[cfg(not(feature = "web"))]
pub mod report;
#[cfg(not(feature = "web"))]
pub mod scene_ops;
#[cfg(not(feature = "web"))]
pub mod scene_preview;
#[cfg(not(feature = "web"))]
pub mod screen_share;
#[cfg(not(feature = "web"))]
pub mod settings_compat;
#[cfg(not(feature = "web"))]
pub mod store_ops;
#[cfg(not(feature = "web"))]
pub mod supervisor;
#[cfg(not(feature = "web"))]
pub mod ui_layout;
#[cfg(not(feature = "web"))]
pub mod video_sources;
#[cfg(not(feature = "web"))]
pub mod vmodv;
#[cfg(not(feature = "web"))]
pub mod window;
#[cfg(not(feature = "web"))]
pub mod window_state;
#[cfg(not(feature = "web"))]
pub mod windows;

#[cfg(not(feature = "web"))]
use std::process::ExitCode;

#[cfg(not(feature = "web"))]
use modv_render::{Engine, EngineConfig};

#[cfg(not(feature = "web"))]
pub fn run_engine() -> ExitCode {
    die_with_parent();

    let cli = cli::parse();

    // WS-9: device enumeration is a pure host query — answer it before any GPU,
    // window or socket work so `--list-audio-devices` works headless and in CI.
    if cli.list_audio_devices {
        audio::print_devices();
        return ExitCode::SUCCESS;
    }

    // Fixture emission is a pure store session: no GPU device, no window and no
    // control socket are touched, so it runs before `Engine::new`.
    if let Some(dir) = &cli.emit_fixtures {
        return match fixtures::emit(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: could not emit fixtures: {err}");
                ExitCode::FAILURE
            }
        };
    }

    if cli.socket.is_some() && cli.headless {
        eprintln!("engine: note — the control socket is only polled in windowed mode");
    }
    if cli.audio_source.engine_owned() && (cli.headless || cli.screenshot.is_some()) {
        eprintln!(
            "engine: note — --audio-source {} is only wired in windowed mode; headless renders against --audio-fixture",
            cli.audio_source.as_str()
        );
    }

    let engine = match Engine::new(&EngineConfig::default()) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let adapter = report::AdapterSummary::from_engine(&engine);
    eprintln!(
        "adapter: {} ({}) vendor={:#06x} driver={}",
        adapter.name, adapter.backend, adapter.vendor, adapter.driver
    );
    if cli.log_adapter {
        match serde_json::to_string_pretty(&adapter.to_json()) {
            Ok(text) => eprintln!("adapter info (json): {text}"),
            Err(err) => eprintln!("error: failed to serialise adapter info: {err}"),
        }
    }
    if cli.print_adapter {
        match serde_json::to_string_pretty(&adapter.to_json()) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("error: failed to serialise adapter info: {err}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if cli.headless || cli.screenshot.is_some() {
        return match headless::run(cli, engine, adapter) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(problem) = x11_backend_problem() {
        eprintln!("engine: ERROR {problem}");
        return ExitCode::FAILURE;
    }

    match window::run(cli, engine, adapter) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Entry point for the `modv` VmodV UI binary.
///
/// `--headless` and `--screenshot` keep the engine-only behaviour.
#[cfg(not(feature = "web"))]
pub fn vmodv_main() -> ExitCode {
    die_with_parent();

    let mut cli = cli::parse();
    cli.bench_seconds = cli::vmodv_bench_seconds(cli.bench_seconds_explicit, cli.bench_seconds);

    if cli.list_audio_devices {
        audio::print_devices();
        return ExitCode::SUCCESS;
    }

    if let Some(dir) = &cli.emit_fixtures {
        return match fixtures::emit(dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: could not emit fixtures: {err}");
                ExitCode::FAILURE
            }
        };
    }

    if cli.socket.is_some() && (cli.headless || cli.screenshot.is_some()) {
        eprintln!("ui: note — the control socket is not served in headless/screenshot mode");
    }

    let engine = match Engine::new(&EngineConfig::default()) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let adapter = report::AdapterSummary::from_engine(&engine);
    eprintln!(
        "adapter: {} ({}) vendor={:#06x} driver={}",
        adapter.name, adapter.backend, adapter.vendor, adapter.driver
    );
    if cli.print_adapter {
        match serde_json::to_string_pretty(&adapter.to_json()) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("error: failed to serialise adapter info: {err}"),
        }
        return ExitCode::SUCCESS;
    }

    if cli.headless || cli.screenshot.is_some() {
        return match headless::run(cli, engine, adapter) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(problem) = x11_backend_problem() {
        eprintln!("ui: ERROR {problem}");
        return ExitCode::FAILURE;
    }

    match vmodv::run(cli, engine, adapter) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Preflight for the X11 fallback: winit loads `libxkbcommon-x11` at runtime
/// (via xkbcommon-dl) and panics with an unhelpful backtrace when it is
/// missing. Detect the situation up front and fail with an actionable line.
///
/// Only applies when the session is X11: the Wayland path needs
/// `libxkbcommon` only, which is a hard dependency of the windowing stack.
#[cfg(all(not(feature = "web"), target_os = "linux"))]
fn x11_backend_problem() -> Option<String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_none() {
        return None;
    }

    for name in ["libxkbcommon-x11.so.0", "libxkbcommon-x11.so"] {
        let Ok(candidate) = std::ffi::CString::new(name) else {
            continue;
        };
        // SAFETY: dlopen() with a valid NUL-terminated name; the handle from
        // dlopen is closed immediately and never dereferenced.
        let handle =
            unsafe { libc::dlopen(candidate.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            // SAFETY: `handle` came from dlopen above.
            unsafe { libc::dlclose(handle) };
            return None;
        }
    }

    Some(
        "the X11 fallback requires libxkbcommon-x11 (e.g. the libxkbcommon-x11-0 \
         package); install it or run the engine under Wayland"
            .to_string(),
    )
}

#[cfg(all(not(feature = "web"), not(target_os = "linux")))]
fn x11_backend_problem() -> Option<String> {
    None
}

/// Ask the kernel to SIGTERM this process when its parent dies.
///
/// The Electron supervisor normally sends `engine/shutdown` on quit, but if
/// the main process is SIGKILLed or the container is torn down abruptly the
/// engine would otherwise be orphaned (it owns a native window and GPU
/// device). `PR_SET_PDEATHSIG` ties its lifetime to the spawner; the periodic
/// snapshot is the state-restore safety net in that path.
#[cfg(all(not(feature = "web"), target_os = "linux"))]
fn die_with_parent() {
    // SAFETY: prctl(PR_SET_PDEATHSIG, SIGTERM) only sets a per-process signal
    // disposition; it has no memory-safety implications and cannot fail in a
    // way that would leave this process in an invalid state.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }
}

#[cfg(all(not(feature = "web"), not(target_os = "linux")))]
fn die_with_parent() {}
