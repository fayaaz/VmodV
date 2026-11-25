//! Process-level harness for the VmodV Rust shell (`modv`).
//!
//! The Electron e2e suite drives a live app through Playwright; the VmodV
//! equivalent launches the real binary (no mocks, no in-process `VmodvApp`)
//! with a temporary userData tree (or an explicit one for restart tests), a
//! JSON-lines control socket and a virtual display when the environment has
//! none. Tests then assert `state`, dispatch Vuex-style actions, capture
//! compositor frames and verify the process exit.
//!
//! ```ignore
//! use crate::e2e::VmodvHarness;
//!
//! let mut app = VmodvHarness::spawn();
//! app.wait_ready(Duration::from_secs(60));
//! app.dispatch("groups/createGroup", serde_json::json!({"name": "E2E"}));
//! assert!(app.state()["groups"].as_array().unwrap().iter().any(|g| g["name"] == "E2E"));
//! app.quit();
//! app.assert_exit(0);
//! ```
//!
//! The harness is deliberately blocking and `Drop`-safe: a panic kills the
//! child and keeps the temp directory, so a failed CI run leaves the app log
//! and the frame PNG behind for diagnosis. `MODV_E2E_KEEP=1` keeps artifacts
//! on success too.

#![cfg(unix)] // this harness drives the Unix-domain control socket
#![allow(dead_code)] // Each integration-test crate uses a different subset.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Default time allowed for the shell to answer `ping`.
pub const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// Default time allowed for one control-socket request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Default time allowed for a graceful quit.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Builder for [`VmodvHarness`].
pub struct VmodvHarnessBuilder {
    width: u32,
    height: u32,
    args: Vec<String>,
    keep: bool,
    user_data: Option<PathBuf>,
}

impl Default for VmodvHarnessBuilder {
    fn default() -> Self {
        Self {
            width: 960,
            height: 540,
            args: Vec::new(),
            keep: std::env::var("MODV_E2E_KEEP").is_ok_and(|value| value == "1"),
            user_data: None,
        }
    }
}

impl VmodvHarnessBuilder {
    /// Output window width (the compositor's main texture size).
    pub fn width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    /// Output window height.
    pub fn height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    /// Append a CLI argument (for `--apply-scene`, `--preset`, `--frames`, …).
    pub fn arg(mut self, argument: impl Into<String>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Apply this Scene name/id after seeding (`--apply-scene`).
    pub fn apply_scene(mut self, scene: impl Into<String>) -> Self {
        self.args.push("--apply-scene".to_string());
        self.args.push(scene.into());
        self
    }

    /// Open a preset/Scene/playlist document at startup (`--preset`).
    pub fn preset(mut self, path: impl AsRef<Path>) -> Self {
        self.args.push("--preset".to_string());
        self.args.push(path.as_ref().to_string_lossy().into_owned());
        self
    }

    /// Start performance mode after startup (`--start-performance`).
    pub fn start_performance(mut self) -> Self {
        self.args.push("--start-performance".to_string());
        self
    }

    /// Keep the temp directory on success (failed tests always keep it).
    pub fn keep(mut self, keep: bool) -> Self {
        self.keep = keep;
        self
    }

    /// Reuse an explicit `<userData>` tree instead of a per-launch temp one.
    ///
    /// Restart tests spawn a sequence of shells over the same tree (state,
    /// Scenes, window placement), so the caller owns it: create it with
    /// [`temp_root`] and remove it when the test finishes. The harness never
    /// deletes an explicit userData directory.
    pub fn user_data(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_data = Some(path.into());
        self
    }

    /// Spawn the VmodV shell and wait until it answers `ping`.
    pub fn spawn(self) -> VmodvHarness {
        VmodvHarness::launch(self)
    }
}

/// A running `modv` process plus its control-socket client.
pub struct VmodvHarness {
    child: Option<Child>,
    client: Option<ControlClient>,
    display: TestDisplay,
    temp_dir: PathBuf,
    user_data: PathBuf,
    socket_path: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    next_id: u64,
    keep: bool,
    width: u32,
    height: u32,
    pid: u32,
}

impl VmodvHarness {
    /// Spawn with the default 960 × 540 output.
    pub fn spawn() -> Self {
        Self::builder().spawn()
    }

    /// Configure a harness before spawning.
    pub fn builder() -> VmodvHarnessBuilder {
        VmodvHarnessBuilder::default()
    }

    fn launch(options: VmodvHarnessBuilder) -> Self {
        let temp_dir = unique_dir("vmodv-e2e");
        fs::create_dir_all(&temp_dir).expect("create harness temp dir");
        let user_data = match options.user_data.clone() {
            Some(path) => path,
            None => temp_dir.join("userData"),
        };
        let home = temp_dir.join("home");
        let runtime_dir = temp_dir.join("runtime");
        fs::create_dir_all(&user_data).expect("create userData dir");
        fs::create_dir_all(&home).expect("create home dir");
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        set_private_dir(&runtime_dir);

        let socket_path = temp_dir.join("control.sock");
        let state_path = user_data.join("engine-state.json");
        let log_path = temp_dir.join("modv.log");
        let log = fs::File::create(&log_path).expect("create modv log");
        let log_err = log.try_clone().expect("clone modv log handle");

        let display = TestDisplay::ensure(&temp_dir);

        let mut command = Command::new(env!("CARGO_BIN_EXE_modv"));
        command
            .arg("--socket")
            .arg(&socket_path)
            .arg("--width")
            .arg(options.width.to_string())
            .arg("--height")
            .arg(options.height.to_string())
            .args(&options.args)
            .env("MODV_USER_DATA", &user_data)
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("RUST_BACKTRACE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        // Isolate the run from the developer's shell: only the temp userData
        // and display selection may influence the shell.
        for key in [
            "MODV_AUDIO_SOURCE",
            "MODV_AUDIO_DEVICE",
            "MODV_ISF_DIR",
            "MODV_USER_ISF_DIR",
            "MODV_MEDIA_DIR",
            "MODV_ENGINE",
            "MODV_UI",
        ] {
            command.env_remove(key);
        }
        display.apply(&mut command);

        let child = command.spawn().unwrap_or_else(|error| {
            panic!(
                "could not launch {}: {error}\n--- display ---\n{}",
                env!("CARGO_BIN_EXE_modv"),
                display.log()
            )
        });
        let pid = child.id();

        Self {
            child: Some(child),
            client: None,
            display,
            temp_dir,
            user_data,
            socket_path,
            state_path,
            log_path,
            next_id: 1,
            keep: options.keep,
            width: options.width,
            height: options.height,
            pid,
        }
    }

    /// The process id of the running shell.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The temporary `<userData>` tree the shell was launched with.
    pub fn user_data(&self) -> &Path {
        &self.user_data
    }

    /// `<userData>/engine-state.json` (written on quit).
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// The control socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The harness temp directory (logs, screenshots, display log).
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// The shell's combined stdout/stderr log.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// The shell's stdout/stderr, or an explanatory note when it is empty.
    pub fn logs(&self) -> String {
        match fs::read_to_string(&self.log_path) {
            Ok(text) if text.is_empty() => "(modv log is empty)".to_string(),
            Ok(text) => text,
            Err(error) => format!("(could not read {}: {error})", self.log_path.display()),
        }
    }

    /// Wait until `ping` answers (the window/GPU loop is live).
    pub fn wait_ready(&mut self) -> Value {
        self.wait_ready_for(READY_TIMEOUT)
    }

    /// Wait up to `timeout` for `ping`.
    pub fn wait_ready_for(&mut self, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        let mut last_error = String::new();
        while Instant::now() < deadline {
            if let Some(status) = self.try_wait() {
                panic!(
                    "VmodV shell exited during startup ({status})\n--- logs ---\n{}",
                    self.logs()
                );
            }
            match self.call(json!({ "cmd": "ping" })) {
                Ok(response) => return response,
                Err(error) => last_error = error,
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "VmodV shell was not ready after {timeout:?}: {last_error}\n--- logs ---\n{}",
            self.logs()
        );
    }

    /// Send one request (the harness injects the correlation id).
    ///
    /// Returns the response's `result` on `ok: true`, or the response's
    /// `error` string on `ok: false`.
    pub fn call(&mut self, request: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let object = request
            .as_object()
            .ok_or_else(|| "harness request must be a JSON object".to_string())?;
        let mut request = object.clone();
        request.insert("id".to_string(), json!(id));

        if self.client.is_none() {
            match ControlClient::connect(&self.socket_path) {
                Ok(client) => self.client = Some(client),
                Err(error) => {
                    return Err(format!(
                        "could not connect to {}: {error}\n--- logs ---\n{}",
                        self.socket_path.display(),
                        self.logs()
                    ));
                }
            }
        }

        let client = self.client.as_mut().expect("client just connected");
        let response = client
            .call(&Value::Object(request))
            .map_err(|error| format!("control request failed: {error}"))?;
        if response.get("id") != Some(&json!(id)) {
            self.client = None;
            return Err(format!(
                "control response id mismatch (wanted {id}): {response}\n--- logs ---\n{}",
                self.logs()
            ));
        }
        if response.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(response
                .get("error")
                .map(|error| {
                    error
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| error.to_string())
                })
                .unwrap_or_else(|| format!("malformed response: {response}")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    /// `state`: the authoritative store snapshot plus runtime info.
    pub fn state(&mut self) -> Value {
        self.expect(json!({ "cmd": "state" }))
    }

    /// `version`: binary and protocol version.
    pub fn version(&mut self) -> Value {
        self.expect(json!({ "cmd": "version" }))
    }

    /// `ping`: readiness probe.
    pub fn ping(&mut self) -> Value {
        self.expect(json!({ "cmd": "ping" }))
    }

    /// `dispatch`: run a store action and return its JSON result.
    pub fn dispatch(&mut self, action: &str, payload: Value) -> Value {
        self.expect(json!({
            "cmd": "dispatch",
            "action": action,
            "payload": payload,
        }))
    }

    /// `dispatch`, returning the store error instead of panicking.
    pub fn try_dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        self.call(json!({
            "cmd": "dispatch",
            "action": action,
            "payload": payload,
        }))
    }

    /// `screenshot`: write the current compositor frame as a PNG.
    pub fn screenshot(
        &mut self,
        path: impl AsRef<Path>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Value {
        let mut request = json!({
            "cmd": "screenshot",
            "path": path.as_ref().to_string_lossy(),
        });
        if let Some(width) = width {
            request["width"] = json!(width);
        }
        if let Some(height) = height {
            request["height"] = json!(height);
        }
        self.expect(request)
    }

    /// `quit`: graceful shutdown (state/layout/logs are saved).
    pub fn quit(&mut self) -> Value {
        self.expect(json!({ "cmd": "quit" }))
    }

    /// `save`: write `engine-state.json` now (File → Save engine snapshot).
    ///
    /// Crash tests use this to establish the snapshot a supervisor would
    /// restore after the process disappears without a graceful quit.
    pub fn save(&mut self) -> Value {
        self.expect(json!({ "cmd": "save" }))
    }

    /// Drop the persistent client so the next request opens a new connection.
    pub fn disconnect(&mut self) {
        self.client = None;
    }

    /// Wait for a state predicate, polling `state` once per 50 ms.
    pub fn wait_for_state(
        &mut self,
        timeout: Duration,
        mut predicate: impl FnMut(&Value) -> bool,
    ) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.state();
            if predicate(&state) {
                return state;
            }
            if Instant::now() >= deadline {
                panic!(
                    "state predicate did not hold within {timeout:?}\n--- state ---\n{state}\n--- logs ---\n{}",
                    self.logs()
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait for the process to exit and return its status.
    pub fn wait_exit(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait() {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                panic!(
                    "VmodV shell did not exit within {timeout:?}\n--- logs ---\n{}",
                    self.logs()
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Assert the process exited with `code` (and include logs on failure).
    pub fn assert_exit(&mut self, code: i32) -> ExitStatus {
        let status = self.wait_exit(EXIT_TIMEOUT);
        assert_eq!(
            status.code(),
            Some(code),
            "unexpected exit status {status}\n--- logs ---\n{}",
            self.logs()
        );
        status
    }

    /// Kill the shell if it is still alive.
    pub fn kill(&mut self) -> io::Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    /// SIGKILL the shell and return its wait status (crash tests).
    pub fn kill_and_wait(&mut self) -> ExitStatus {
        let mut child = self
            .child
            .take()
            .expect("kill_and_wait needs a running VmodV shell");
        let _ = child.kill();
        child.wait().expect("reap the killed VmodV shell")
    }

    fn expect(&mut self, request: Value) -> Value {
        self.call(request).unwrap_or_else(|error| {
            panic!(
                "control request failed: {error}\n--- logs ---\n{}",
                self.logs()
            )
        })
    }

    fn try_wait(&mut self) -> Option<ExitStatus> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                Some(status)
            }
            Ok(None) => None,
            Err(error) => panic!("could not poll the VmodV shell: {error}"),
        }
    }
}

impl Drop for VmodvHarness {
    fn drop(&mut self) {
        let _ = self.kill();
        // A failing assertion keeps the temp tree (log + PNG) for diagnosis.
        if self.keep || std::thread::panicking() {
            eprintln!(
                "VmodV e2e artifacts kept at {} ({}x{})",
                self.temp_dir.display(),
                self.width,
                self.height
            );
        } else {
            let _ = fs::remove_dir_all(&self.temp_dir);
        }
    }
}

struct ControlClient {
    reader: BufReader<UnixStream>,
}

impl ControlClient {
    fn connect(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
        Ok(Self {
            reader: BufReader::new(stream),
        })
    }

    fn call(&mut self, request: &Value) -> io::Result<Value> {
        let mut line = serde_json::to_string(request)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push('\n');
        self.reader.get_mut().write_all(line.as_bytes())?;
        self.reader.get_mut().flush()?;

        let mut response = String::new();
        if self.reader.read_line(&mut response)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control socket closed before the response",
            ));
        }
        serde_json::from_str(response.trim_end())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

// ————————————————— Virtual display —————————————————

pub(crate) struct TestDisplay {
    kind: DisplayKind,
    child: Option<Child>,
    env: Vec<(String, String)>,
    runtime_dir: Option<PathBuf>,
    log_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayKind {
    External,
    Xvfb,
    Sway,
}

impl TestDisplay {
    pub(crate) fn ensure(temp_dir: &Path) -> Self {
        // Prefer Wayland when the session offers it; X11/XWayland is the
        // fallback. The headless path below stays on Xvfb because the CI
        // image has no Wayland compositor.
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) {
            return Self::external(DisplayKind::External);
        }
        if std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()) {
            return Self::external(DisplayKind::External);
        }

        let log_path = temp_dir.join("display.log");
        match Self::start_xvfb(&log_path) {
            Ok(display) => display,
            Err(xvfb_error) => match Self::start_sway(temp_dir, &log_path) {
                Ok(display) => display,
                Err(sway_error) => panic!(
                    "no display: Xvfb failed ({xvfb_error}); sway --headless failed ({sway_error})\n\
                     install xvfb (or run with DISPLAY/WAYLAND_DISPLAY set)\n--- display log ---\n{}",
                    fs::read_to_string(&log_path).unwrap_or_default()
                ),
            },
        }
    }

    fn external(kind: DisplayKind) -> Self {
        Self {
            kind,
            child: None,
            env: Vec::new(),
            runtime_dir: None,
            log_path: PathBuf::new(),
        }
    }

    fn start_xvfb(log_path: &Path) -> Result<Self, String> {
        let number = free_display_number().ok_or("no free X display number in :90–:140")?;
        let display = format!(":{number}");
        let log = fs::File::create(log_path)
            .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
        let log_err = log
            .try_clone()
            .map_err(|error| format!("could not clone display log: {error}"))?;

        let child = Command::new("Xvfb")
            .arg(&display)
            .args(["-screen", "0", "1920x1080x24", "-nolisten", "tcp", "-ac"])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|error| format!("could not spawn Xvfb: {error}"))?;

        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
        let mut child = child;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("could not poll Xvfb: {error}"))?
            {
                return Err(format!(
                    "Xvfb {display} exited with {status}: {}",
                    fs::read_to_string(log_path).unwrap_or_default()
                ));
            }
            if socket.exists() {
                return Ok(Self {
                    kind: DisplayKind::Xvfb,
                    child: Some(child),
                    env: vec![("DISPLAY".to_string(), display)],
                    runtime_dir: None,
                    log_path: log_path.to_path_buf(),
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Xvfb {display} did not create {}",
                    socket.display()
                ));
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn start_sway(temp_dir: &Path, log_path: &Path) -> Result<Self, String> {
        let runtime_dir = temp_dir.join("sway-runtime");
        fs::create_dir_all(&runtime_dir)
            .map_err(|error| format!("could not create {}: {error}", runtime_dir.display()))?;
        set_private_dir(&runtime_dir);
        let log = fs::File::create(log_path)
            .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
        let log_err = log
            .try_clone()
            .map_err(|error| format!("could not clone display log: {error}"))?;

        let child = Command::new("sway")
            .args(["-c", "/dev/null"])
            .env("WLR_BACKENDS", "headless")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|error| format!("could not spawn sway: {error}"))?;

        let mut child = child;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("could not poll sway: {error}"))?
            {
                return Err(format!(
                    "sway exited with {status}: {}",
                    fs::read_to_string(log_path).unwrap_or_default()
                ));
            }
            if let Some(wayland) = first_wayland_socket(&runtime_dir) {
                return Ok(Self {
                    kind: DisplayKind::Sway,
                    child: Some(child),
                    env: vec![("WAYLAND_DISPLAY".to_string(), wayland)],
                    runtime_dir: Some(runtime_dir),
                    log_path: log_path.to_path_buf(),
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("sway did not create a Wayland socket".to_string());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    pub(crate) fn apply(&self, command: &mut Command) {
        match self.kind {
            DisplayKind::External => {
                // The harness launches the shell with its private temp
                // XDG_RUNTIME_DIR (sway/Xvfb need a private socket dir).
                // For the host's Wayland compositor the socket lives in the
                // host's runtime directory, so an external Wayland display
                // must see that one — otherwise the shell can never find
                // the compositor. (X11 works either way: X clients look in
                // /tmp/.X11-unix, not XDG_RUNTIME_DIR.)
                if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
                    && let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR")
                {
                    command.env("XDG_RUNTIME_DIR", xdg);
                    command.env("XDG_SESSION_TYPE", "wayland");
                    // Both can be set on a Wayland desktop (XWayland). Drop
                    // DISPLAY so winit cannot silently pick the X11 backend.
                    command.env_remove("DISPLAY");
                }
            }
            _ => {
                command.env_remove("DISPLAY");
                command.env_remove("WAYLAND_DISPLAY");
                for (key, value) in &self.env {
                    command.env(key, value);
                }
            }
        }
        if self.kind == DisplayKind::Xvfb {
            command.env("XDG_SESSION_TYPE", "x11");
        }
        if self.kind == DisplayKind::Sway {
            command.env("XDG_SESSION_TYPE", "wayland");
        }
    }

    fn log(&self) -> String {
        if self.log_path.as_os_str().is_empty() {
            "(external display; no display log)".to_string()
        } else {
            fs::read_to_string(&self.log_path).unwrap_or_default()
        }
    }
}

impl Drop for TestDisplay {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(dir) = &self.runtime_dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn first_wayland_socket(runtime_dir: &Path) -> Option<String> {
    let entries = fs::read_dir(runtime_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("wayland-") && !name.ends_with(".lock") {
            return Some(name);
        }
    }
    None
}

fn free_display_number() -> Option<u32> {
    let start = 90 + (std::process::id() % 20);
    (start..start + 50).find(|number| {
        !Path::new(&format!("/tmp/.X11-unix/X{number}")).exists()
            && !Path::new(&format!("/tmp/.X{number}-lock")).exists()
    })
}

// ————————————————— Filesystem helpers —————————————————

fn unique_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("modv-{prefix}-{}-{nanos}", std::process::id()))
}

/// Create a unique temp directory for a multi-launch test.
///
/// The harness removes only its own per-launch tree, so a restart test owns
/// this root: pass `root.join("userData")` to
/// [`VmodvHarnessBuilder::user_data`] and remove `root` when the test ends.
pub fn temp_root(prefix: &str) -> PathBuf {
    let dir = unique_dir(prefix);
    fs::create_dir_all(&dir).expect("create harness temp root");
    dir
}

fn set_private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}
