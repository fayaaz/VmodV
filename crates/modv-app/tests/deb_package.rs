//! RED guards for per-Ubuntu `.deb` packages published from the release
//! workflow (slug `ubuntu-deb-matrix`).
//!
//! One generic `VmodV-<version>-amd64.deb` cannot be assumed to work on every
//! Ubuntu release: the release must build one `.deb` per supported Ubuntu base
//! and give each a distinct suffix so four artifacts can coexist on a single
//! GitHub release. The default unsuffixed `.deb` is retired — the release
//! publishes the AppImage plus the four Ubuntu debs.
//!
//! ## The `tools/vmodv-deb.sh --dry-run` contract enforced here
//!
//! `bash tools/vmodv-deb.sh` must accept the following flags. Bracketed values
//! are the real-mode defaults; a dry run supplies every one of them explicitly
//! so it never needs a cargo build, a real binary, git tags or `dpkg-deb`:
//!
//! ```text
//! --dry-run              stage only; run NO cargo build, NO dpkg-deb and NO
//!                        tools/vmodv-deb-check.sh invocation
//! --project-root DIR     repository root [ROOT, default /app]
//! --binary FILE          modv binary to stage
//!                        [<project-root>/crates/target/release/modv]
//! --dist DIR             output directory [<project-root>/dist]
//! --version VER          package version
//!                        [APP_VERSION / VMODV_VERSION / tools/version.sh]
//! --suffix SUFFIX        appended verbatim to the artifact name [empty];
//!                        real mode also reads DEB_SUFFIX from the environment
//! ```
//!
//! The artifact name is `VmodV-<version>-amd64<suffix>.deb`. Thus
//! `--version 9.9.9 --suffix -ubuntu26.04` produces
//! `VmodV-9.9.9-amd64-ubuntu26.04.deb`. **Dry-run stdout must contain that
//! exact name** plus a short summary (staged file count / installed size) so a
//! caller can assert which artifact would be built.
//!
//! A dry run stages a package tree under `--dist` from `--project-root`'s
//! `packaging/vmodv/*`, `resources/*`, `LICENSE` and
//! `THIRD_PARTY_NOTICES.md` fixtures. The natural path
//! is `<dist>/VmodV-<version>-amd64<suffix>/DEBIAN/control`, but these tests
//! locate any `DEBIAN/control` below `--dist`. The staged control must have
//! `Version: <ver>` verbatim, a numeric `Installed-Size` greater than zero and
//! the SAME 12 host `Depends` names as `packaging/vmodv/vmodv.control` — the
//! dependency list is distro-independent (Ubuntu's t64 packages provide the
//! legacy names), so no per-distro control templating is allowed.
//!
//! In real mode `DEB_SUFFIX` is forwarded to `tools/vmodv-deb-check.sh`, which
//! derives its artifact path from `${DEB_SUFFIX}` while leaving its 12 Depends
//! assertions unchanged.
//!
//! The `Dockerfile` keeps `ARG DEBIAN_RELEASE=trixie` as the default and gains
//! `ARG BASE_IMAGE=debian:${DEBIAN_RELEASE}-slim` /
//! `FROM ${BASE_IMAGE} AS base` so only the release workflow's deb matrix
//! injects Ubuntu bases. No Ubuntu base literal, no `UBUNTU_RELEASE` and no
//! t64 PipeWire rename belong in the Dockerfile.
//!
//! ## Project licence and attribution payload (slug `REL-05`)
//!
//! The package must install the repository `LICENSE` as
//! `usr/share/doc/vmodv/copyright` and `THIRD_PARTY_NOTICES.md` beside it. The
//! notices preserve the Point10 / Shadertoy attribution and CC BY-NC-SA terms.
//! Both artifact checkers must require those paths and grep the notices for the
//! `Point10` and `CC BY-NC-SA` markers; the AppImage checker also has behavioural
//! coverage in `appimage_bundle.rs`.
//!
//! ## amd64-only `.deb` architecture guards (slug `REL-06`)
//!
//! `tools/vmodv-deb.sh` must reject non-x86_64/amd64 hosts before building and
//! validate the ELF machine of any provided or built binary, including in
//! `--dry-run --binary FILE` mode. `tools/elf-machine.sh FILE` is the shared,
//! std-only header reader used by the producer and checker; it prints `x86_64`,
//! `aarch64` or `unknown` for an ELF file and exits non-zero for missing or
//! non-ELF input. `tools/vmodv-deb-check.sh` must inspect the extracted
//! `usr/lib/vmodv/modv` and require `x86_64`, rather than trusting the package's
//! `Architecture: amd64` metadata alone.
//!
//! `make build` must select only the native AppImage on non-amd64 hosts, while
//! an explicit `make vmodv-deb` still reaches the script and fails fast.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test deb_package -- --test-threads=1"
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Flags the deb script must advertise (see the module doc for their contract).
const DEB_FLAGS: [&str; 6] = [
    "--dry-run",
    "--project-root",
    "--binary",
    "--dist",
    "--version",
    "--suffix",
];

/// Attribution fixture staged into the fake project root and expected in the
/// package's documentation directory.
const FAKE_THIRD_PARTY_NOTICES: &str =
    "VmodV third-party notices\nPoint10 material is licensed under CC BY-NC-SA.\n";

/// The 12 host runtime dependencies in `packaging/vmodv/vmodv.control` and in
/// `tools/vmodv-deb-check.sh`. One list serves Debian and every Ubuntu leg.
const DEPENDS_NAMES: [&str; 12] = [
    "libc6",
    "libasound2",
    "libpipewire-0.3-0",
    "libgstreamer1.0-0",
    "libgstreamer-plugins-base1.0-0",
    "libglib2.0-0",
    "libvulkan1",
    "libxkbcommon-x11-0",
    "gstreamer1.0-plugins-base",
    "gstreamer1.0-plugins-good",
    "gstreamer1.0-libav",
    "gstreamer1.0-pipewire",
];

/// (base image, artifact suffix) for every leg of the `linux-debs` matrix.
const DEB_MATRIX: [(&str, &str); 4] = [
    ("ubuntu:24.04", "-ubuntu24.04"),
    ("ubuntu:26.04", "-ubuntu26.04"),
    ("ubuntu:26.10", "-ubuntu26.10"),
    ("ubuntu:devel", "-ubuntudevel"),
];

/// A stand-in for `cargo` / `dpkg-deb`: records every invocation, so a dry run
/// that accidentally shells out to either tool is detectable even when the
/// shim itself exits successfully.
const FORBIDDEN_TOOL_SHIM: &str = concat!(
    "#!/bin/sh\n",
    "printf '%s\\n' \"${0##*/}\" >> \"$DRY_RUN_FORBIDDEN_LOG\"\n",
    "exit 0\n",
);

fn unique_dir_name(prefix: &str) -> String {
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{}-{nanos}-{counter}", std::process::id())
}

/// Minimal RAII guard: removes the whole tree on drop, including on panic.
struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(name);
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove a stale temp dir from a previous run");
        }
        fs::create_dir_all(&path).expect("create the temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn assert_contains(text: &str, needle: &str, context: &str) {
    assert!(
        text.contains(needle),
        "{context} does not contain {needle:?}:\n{text}"
    );
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("cannot chmod {}: {error}", path.display()));
}

/// A minimal 64-bit little-endian ELF header. Only the identification bytes and
/// `e_machine` are populated because `tools/elf-machine.sh` must not depend on
/// `readelf`, `file` or a runnable binary.
fn write_elf_header(path: &Path, machine: u16) {
    let mut header = [0_u8; 64];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = 2; // ELFCLASS64
    header[5] = 1; // ELFDATA2LSB
    header[6] = 1; // EV_CURRENT
    header[18..20].copy_from_slice(&machine.to_le_bytes());
    fs::write(path, header)
        .unwrap_or_else(|error| panic!("cannot write ELF fixture {}: {error}", path.display()));
}

/// Shadow only `uname -m`; every other invocation reaches the real system tool.
fn write_uname_shim(dir: &Path, machine: &str) {
    fs::create_dir_all(dir)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
    let contents = format!(
        "#!/bin/sh\n\
         case \"${{1:-}}\" in\n\
         \t-m|--machine) printf '%s\\n' '{machine}'; exit 0 ;;\n\
         \tesac\n\
         exec /usr/bin/uname \"$@\"\n"
    );
    write_executable(&dir.join("uname"), &contents);
}

/// Recursively collect every regular file under `dir` (std-only, no walkdir).
fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read directory {}: {error}", dir.display()));
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => panic!("cannot read an entry in {}: {error}", dir.display()),
        };
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

/// Refuse to launch the current script for real: at RED it ignores unknown
/// flags and would otherwise run `cargo build` + `dpkg-deb` with the ambient
/// environment. This pre-assertion makes the missing `--dry-run` mode fail as
/// a readable assertion instead.
fn assert_dry_run_flags(script_text: &str) {
    for flag in DEB_FLAGS {
        assert!(
            script_text.contains(flag),
            "tools/vmodv-deb.sh does not accept {flag}: the per-Ubuntu .deb dry-run \
             contract is not implemented (see the module doc of \
             crates/modv-app/tests/deb_package.rs for the exact flags)"
        );
    }
}

/// The body of a Makefile target: its tab-indented recipe lines. Accepts both
/// a bare `target:` line and a normal rule with prerequisites
/// (`target: prereq1 prereq2`), so the Makefile need not be split into
/// duplicate rules purely to satisfy this helper.
fn recipe(text: &str, target: &str) -> String {
    let key = format!("{target}:");
    let with_prereqs = format!("{key} ");
    let lines: Vec<&str> = text.lines().collect();
    let start = match lines.iter().position(|line| {
        let line = line.trim_end();
        line == key || line.starts_with(&with_prereqs)
    }) {
        Some(index) => index,
        None => return String::new(),
    };
    let mut body = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if line.starts_with('\t') {
            body.push(*line);
        } else if line.trim().is_empty() {
            continue;
        } else {
            break;
        }
    }
    body.join("\n")
}

/// The body of a top-level GitHub Actions job (two-space-indented key).
fn job_block(text: &str, job: &str) -> Option<String> {
    let key = format!("  {job}:");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| *line == key)?;
    let mut end = lines.len();
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        if line.starts_with("  ") && !line.starts_with("   ") && line.contains(':') {
            end = offset;
            break;
        }
    }
    Some(lines[start + 1..end].join("\n"))
}

fn needs_line(block: &str) -> String {
    block
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("needs:"))
        .unwrap_or("")
        .to_string()
}

#[test]
fn deb_script_advertises_the_dry_run_contract() {
    let script = repo_root().join("tools/vmodv-deb.sh");
    assert!(
        script.is_file(),
        "{} is missing: there is no .deb build script",
        script.display()
    );
    let text = read(&script);
    assert_dry_run_flags(&text);
    assert_contains(
        &text,
        "DEB_SUFFIX",
        "tools/vmodv-deb.sh (env fallback for --suffix in real mode)",
    );
}

/// Build the minimal fake project used by the `.deb` dry-run tests and invoke
/// the producer without cargo, dpkg-deb or the checker.
fn stage_deb_dry_run(base: &Path) -> Output {
    stage_deb_dry_run_with_path_prefix(base, None)
}

/// Variant that puts an additional directory (for example an architecture
/// `uname` shim) ahead of the forbidden cargo/dpkg-deb shims.
fn stage_deb_dry_run_with_path_prefix(base: &Path, path_prefix: Option<&Path>) -> Output {
    let repo_root = repo_root();

    // Minimal fake `--project-root`: the real control template (so the 12
    // Depends names are exact) and stubs for every other payload input.
    let project = base.join("project");
    let packaging = project.join("packaging/vmodv");
    let isf = project.join("resources/isf");
    let textures = project.join("resources/point10/textures");
    for dir in [&packaging, &isf, &textures] {
        fs::create_dir_all(dir)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
    }
    fs::copy(
        repo_root.join("packaging/vmodv/vmodv.control"),
        packaging.join("vmodv.control"),
    )
    .expect("copy the real vmodv.control into the fake project root");
    for name in ["deb-launcher.sh", "log-run.sh", "vmodv.desktop"] {
        fs::copy(
            repo_root.join("packaging/vmodv").join(name),
            packaging.join(name),
        )
        .unwrap_or_else(|error| panic!("cannot copy packaging input {name}: {error}"));
    }
    fs::write(isf.join("Bloom.fs"), "fake ISF shader\n").expect("write a fake ISF file");
    fs::write(textures.join("stub.png"), "fake texture\n").expect("write a fake texture");
    fs::write(project.join("resources/icon.png"), "fake icon\n").expect("write a fake icon");
    fs::write(project.join("LICENSE"), "fake licence\n").expect("write a fake LICENSE");
    fs::write(
        project.join("THIRD_PARTY_NOTICES.md"),
        FAKE_THIRD_PARTY_NOTICES,
    )
    .expect("write fake third-party notices");

    let binary = base.join("modv-stub");
    write_elf_header(&binary, 0x003e); // EM_X86_64
    let dist = base.join("dist");

    // `cargo` and `dpkg-deb` are forbidden in a dry run. Put recording shims
    // first on PATH so any invocation is observable.
    let shim_dir = base.join("shims");
    fs::create_dir_all(&shim_dir).expect("create the shim directory");
    write_executable(&shim_dir.join("cargo"), FORBIDDEN_TOOL_SHIM);
    write_executable(&shim_dir.join("dpkg-deb"), FORBIDDEN_TOOL_SHIM);
    let forbidden_log = base.join("forbidden-tools.log");
    let inherited_path = std::env::var_os("PATH")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = match path_prefix {
        Some(prefix) => format!(
            "{}:{}:{inherited_path}",
            prefix.display(),
            shim_dir.display()
        ),
        None => format!("{}:{inherited_path}", shim_dir.display()),
    };

    let script = repo_root.join("tools/vmodv-deb.sh");
    let script_text = read(&script);
    assert_dry_run_flags(&script_text);

    Command::new("bash")
        .arg(&script)
        .arg("--dry-run")
        .arg("--project-root")
        .arg(&project)
        .arg("--binary")
        .arg(&binary)
        .arg("--dist")
        .arg(&dist)
        .arg("--version")
        .arg("9.9.9")
        .arg("--suffix")
        .arg("-ubuntu26.04")
        .env("PATH", path)
        .env("DRY_RUN_FORBIDDEN_LOG", &forbidden_log)
        .env_remove("APP_VERSION")
        .env_remove("VMODV_VERSION")
        .current_dir(&repo_root)
        .output()
        .expect("spawn bash tools/vmodv-deb.sh --dry-run")
}

#[test]
fn dry_run_stages_control_with_distro_suffix() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-deb-matrix"));
    let base = holder.path();
    let dist = base.join("dist");
    let output = stage_deb_dry_run(base);

    assert!(
        output.status.success(),
        "tools/vmodv-deb.sh --dry-run failed with status {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VmodV-9.9.9-amd64-ubuntu26.04.deb"),
        "dry-run stdout must name the suffixed artifact \
         VmodV-9.9.9-amd64-ubuntu26.04.deb:\n{stdout}"
    );

    let invoked = fs::read_to_string(base.join("forbidden-tools.log")).unwrap_or_default();
    assert!(
        invoked.trim().is_empty(),
        "the deb dry-run must not invoke cargo or dpkg-deb, but it invoked: \
         {invoked:?}"
    );

    let mut staged_files = Vec::new();
    collect_files(&dist, &mut staged_files);
    let control = staged_files.iter().find(|path| {
        path.file_name().and_then(|value| value.to_str()) == Some("control")
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|value| value.to_str())
                == Some("DEBIAN")
    });
    assert!(
        control.is_some(),
        "the dry run did not stage a DEBIAN/control below {}; staged files: \
         {staged_files:#?}",
        dist.display()
    );
    let control = control.expect("checked above");
    let control_text = read(control);

    let version = control_text
        .lines()
        .find(|line| line.starts_with("Version:"));
    assert_eq!(
        version,
        Some("Version: 9.9.9"),
        "staged DEBIAN/control has the wrong version:\n{control_text}"
    );

    let installed_size = match control_text
        .lines()
        .find(|line| line.starts_with("Installed-Size:"))
    {
        Some(line) => line["Installed-Size:".len()..].trim().parse::<u64>().ok(),
        None => None,
    };
    assert!(
        matches!(installed_size, Some(size) if size > 0),
        "staged DEBIAN/control must have a numeric, non-zero Installed-Size:\n{control_text}"
    );

    let depends = match control_text
        .lines()
        .find(|line| line.starts_with("Depends:"))
    {
        Some(line) => line["Depends:".len()..].trim(),
        None => "",
    };
    let names: BTreeSet<&str> = depends
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(
        names.len(),
        DEPENDS_NAMES.len(),
        "staged control must keep exactly the unchanged 12-name Depends list; \
         got {names:?}"
    );
    for dep in DEPENDS_NAMES {
        assert!(
            names.contains(dep),
            "staged DEBIAN/control Depends is missing {dep}: {depends}"
        );
    }
}

/// The package's documentation directory must carry VmodV's own licence and
/// third-party attribution payload, not just dependency metadata (REL-05).
#[test]
fn dry_run_stages_project_licence_and_third_party_notices() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-deb-project-docs"));
    let base = holder.path();
    let dist = base.join("dist");
    let output = stage_deb_dry_run(base);
    assert!(
        output.status.success(),
        "tools/vmodv-deb.sh --dry-run failed with status {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let mut staged_files = Vec::new();
    collect_files(&dist, &mut staged_files);
    let control = staged_files.iter().find(|path| {
        path.file_name().and_then(|value| value.to_str()) == Some("control")
            && path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|value| value.to_str())
                == Some("DEBIAN")
    });
    let control = control.expect("the dry run must stage DEBIAN/control");
    let package_root = control
        .parent()
        .and_then(Path::parent)
        .expect("DEBIAN/control has a package root");
    let docs = package_root.join("usr/share/doc/vmodv");

    let copyright = docs.join("copyright");
    assert!(
        copyright.is_file(),
        "{} is not a file: the .deb must install the project LICENSE as its copyright text",
        copyright.display()
    );
    assert_eq!(
        read(&copyright),
        "fake licence\n",
        "{} must be copied from the fake project LICENSE",
        copyright.display()
    );

    let notices = docs.join("THIRD_PARTY_NOTICES.md");
    assert!(
        notices.is_file(),
        "{} is not a file: the .deb must install VmodV's third-party notices",
        notices.display()
    );
    let notices_text = read(&notices);
    assert_eq!(
        notices_text,
        FAKE_THIRD_PARTY_NOTICES,
        "{} must be copied from the fake project's THIRD_PARTY_NOTICES.md",
        notices.display()
    );
    for marker in ["Point10", "CC BY-NC-SA"] {
        assert!(
            notices_text.contains(marker),
            "{} is missing the {marker:?} attribution marker",
            notices.display()
        );
    }
}

#[test]
fn checker_uses_the_distro_suffix() {
    let checker = repo_root().join("tools/vmodv-deb-check.sh");
    assert!(
        checker.is_file(),
        "{} is missing: there is no .deb checker",
        checker.display()
    );
    let text = read(&checker);

    assert!(
        text.lines()
            .any(|line| line.trim_start().starts_with("DEB_SUFFIX=")),
        "tools/vmodv-deb-check.sh must give DEB_SUFFIX an empty default:\n{text}"
    );
    assert_contains(
        &text,
        "${DEB_SUFFIX:-}",
        "tools/vmodv-deb-check.sh (DEB_SUFFIX default)",
    );
    assert_contains(
        &text,
        "VmodV-$VERSION-amd64${DEB_SUFFIX}.deb",
        "tools/vmodv-deb-check.sh (suffixed DEB derivation)",
    );

    // The dependency assertions must stay unchanged: one Depends list serves
    // Debian and all four Ubuntu bases.
    for dep in DEPENDS_NAMES {
        assert_contains(&text, dep, "tools/vmodv-deb-check.sh Depends assertions");
    }
}

/// Both release gates must require VmodV's project documents and validate the
/// attribution markers, rather than accepting dependency copyrights alone.
#[test]
fn artifact_checkers_require_project_attribution_documents() {
    for script_name in ["tools/vmodv-deb-check.sh", "tools/vmodv-appimage-check.sh"] {
        let script = repo_root().join(script_name);
        assert!(
            script.is_file(),
            "{} is missing: there is no artifact checker",
            script.display()
        );
        let text = read(&script);
        for path in [
            "usr/share/doc/vmodv/copyright",
            "usr/share/doc/vmodv/THIRD_PARTY_NOTICES.md",
        ] {
            assert_contains(
                &text,
                path,
                &format!("{script_name} project attribution payload"),
            );
        }
        for marker in ["Point10", "CC BY-NC-SA"] {
            assert!(
                text.lines()
                    .any(|line| line.contains("grep") && line.contains(marker)),
                "{script_name} does not grep the extracted notices for {marker:?}:\n{text}"
            );
        }
    }
}

#[test]
fn makefile_parameterizes_base_image_and_suffix() {
    let makefile = repo_root().join("Makefile");
    let text = read(&makefile);

    assert!(
        text.lines()
            .any(|line| line == "BASE_IMAGE ?= debian:trixie-slim"),
        "Makefile must default BASE_IMAGE to debian:trixie-slim:\n{text}"
    );
    assert!(
        text.lines().any(|line| line == "DEB_SUFFIX ?="),
        "Makefile must declare an empty DEB_SUFFIX default:\n{text}"
    );

    let docker_rust = recipe(&text, "docker-rust");
    assert_contains(
        &docker_rust,
        "--build-arg \"BASE_IMAGE=$(BASE_IMAGE)\"",
        "the docker-rust recipe",
    );

    for target in ["vmodv-deb", "vmodv-deb-check"] {
        let body = recipe(&text, target);
        assert_contains(
            &body,
            "-e DEB_SUFFIX=$(DEB_SUFFIX)",
            &format!("the {target} recipe"),
        );
    }
}

#[test]
fn dockerfile_defaults_to_trixie_but_allows_a_base_override() {
    let dockerfile = repo_root().join("Dockerfile");
    assert!(
        dockerfile.is_file(),
        "{} is missing: there is no build image to parameterise",
        dockerfile.display()
    );
    let text = read(&dockerfile);

    for required in [
        "ARG DEBIAN_RELEASE=trixie",
        "ARG BASE_IMAGE=debian:${DEBIAN_RELEASE}-slim",
        "FROM ${BASE_IMAGE} AS base",
        "libpipewire-0.3-0 \\",
    ] {
        assert_contains(&text, required, "Dockerfile");
    }

    for forbidden in ["ubuntu:", "UBUNTU_RELEASE", "libpipewire-0.3-0t64"] {
        assert!(
            !text.contains(forbidden),
            "Dockerfile still contains {forbidden:?}: the default base must stay trixie \
             and the legacy PipeWire name must be kept (Ubuntu's t64 packages provide \
             it).\n{text}"
        );
    }
}

#[test]
fn release_workflow_builds_and_publishes_four_ubuntu_debs() {
    let workflow = repo_root().join(".github/workflows/release.yml");
    let text = read(&workflow);

    let linux = job_block(&text, "linux-debs");
    assert!(
        linux.is_some(),
        "release.yml has no `linux-debs` job: no Ubuntu .debs are built for the release"
    );
    let linux = linux.expect("checked above");

    let linux_lines: Vec<&str> = linux.lines().collect();
    for (image, suffix) in DEB_MATRIX {
        assert_contains(&linux, image, "the linux-debs matrix");
        assert_contains(&linux, suffix, "the linux-debs matrix");

        // The image and its suffix must be in the SAME matrix entry, not just
        // somewhere in the job.
        let focus = match linux_lines.iter().position(|line| line.contains(image)) {
            Some(index) => index,
            None => continue, // the contains assertion above reports this
        };
        let start = focus.saturating_sub(5);
        let end = (focus + 6).min(linux_lines.len());
        let window = linux_lines[start..end].join("\n");
        assert!(
            window.contains(suffix),
            "the {image} matrix entry must use the suffix {suffix}; nearby lines:\n{window}"
        );
    }
    assert!(
        !text.contains("ubuntu:27.04"),
        "release.yml must not use the non-existent ubuntu:27.04 tag; use ubuntu:devel \
         for the moving development branch"
    );

    let artifact_names: BTreeSet<&str> = linux
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name: vmodv-deb-").map(str::trim))
        .collect();
    assert!(
        artifact_names.len() >= 4,
        "the four Ubuntu legs must upload four distinct artifacts, got {artifact_names:?}"
    );
    assert_contains(
        &linux,
        "uses: actions/upload-artifact@v4",
        "the linux-debs job",
    );
    assert_contains(&linux, "BASE_IMAGE", "the linux-debs job");
    assert_contains(&linux, "--build-arg", "the linux-debs job");
    assert_contains(&linux, "DEB_SUFFIX", "the linux-debs job");
    assert_contains(&needs_line(&linux), "version", "the linux-debs job's needs");

    let publish = job_block(&text, "publish-debs");
    assert!(
        publish.is_some(),
        "release.yml has no `publish-debs` job: the Ubuntu debs would never be attached \
         to the GitHub release"
    );
    let publish = publish.expect("checked above");
    let publish_needs = needs_line(&publish);
    assert!(
        publish_needs.contains("release") && publish_needs.contains("linux-debs"),
        "publish-debs must wait for both the release and linux-debs jobs so a slow or \
         flaky Ubuntu leg cannot block tagging; got {publish_needs:?}"
    );
    for required in [
        "pattern: vmodv-deb-*",
        "merge-multiple: true",
        "gh release upload",
        "v${VERSION}",
        "dist/VmodV-*.deb",
    ] {
        assert_contains(&publish, required, "the publish-debs job");
    }
    assert!(
        !publish.contains("make build"),
        "publish-debs must not run `make build`: that would rebuild the retired generic \
         .deb; the release job builds the AppImage instead"
    );

    let release_job = job_block(&text, "release");
    assert!(release_job.is_some(), "release.yml has no `release` job");
    let release_job = release_job.expect("checked above");
    let release_needs = needs_line(&release_job);
    assert!(
        !release_needs.contains("linux-debs"),
        "the release job must NOT wait for linux-debs: a flaky ubuntu:devel leg must not \
         block tagging; got {release_needs:?}"
    );
    // The AppImage is built exactly once, in `build-appimage`, and handed to
    // `release` as a workflow artifact. `release` itself must not build it:
    // that would both duplicate the work and gate the release on the build.
    let appimage = job_block(&text, "build-appimage").expect(
        "release.yml has no `build-appimage` job: the AppImage would be built \
         in the `release` job, gating the release on the build",
    );
    assert!(
        appimage.lines().any(|line| {
            let line = line.trim();
            line.starts_with("run:") && line.contains("make appimage-vmodv")
        }),
        "the `build-appimage` job must run `make appimage-vmodv` in a step:\n{appimage}"
    );
    assert!(
        !release_job.contains("make appimage-vmodv"),
        "the `release` job must not build the AppImage: `build-appimage` does \
         that once and uploads it for `attach-appimage`"
    );
    assert!(
        !release_job.contains("make build"),
        "no release job may run `make build`; the generic .deb is retired and \
         installers are built by their own jobs"
    );

    let create_command = match release_job.find("gh release create") {
        Some(index) => &release_job[index..],
        None => "",
    };
    assert!(
        !create_command.is_empty(),
        "the release job must create the GitHub release with `gh release create`"
    );
    // Bound the scan to the `gh release create` invocation itself so later
    // attach steps in this job cannot trip the check.
    let create_command = match create_command.find("\n        - name:") {
        Some(end) => &create_command[..end],
        None => create_command,
    };
    assert!(
        !create_command.contains("dist/"),
        "`gh release create` must publish notes only; artifacts are attached \
         later by the `attach-*` jobs so a failed build cannot stop the release:\n\
         {create_command}"
    );
}

#[test]
fn elf_machine_helper_identifies_supported_arches() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-elf-machine"));
    let base = holder.path();
    let helper = repo_root().join("tools/elf-machine.sh");

    for (machine, expected) in [(0x003e_u16, "x86_64"), (0x00b7_u16, "aarch64")] {
        let elf = base.join(format!("elf-{machine:04x}"));
        write_elf_header(&elf, machine);
        let output = Command::new("bash")
            .arg(&helper)
            .arg(&elf)
            .current_dir(repo_root())
            .output()
            .expect("spawn bash tools/elf-machine.sh");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "tools/elf-machine.sh failed for e_machine {machine:#06x} with status {:?}\n\
             stdout:\n{stdout}\nstderr:\n{stderr}",
            output.status,
        );
        assert_eq!(
            stdout.trim(),
            expected,
            "tools/elf-machine.sh misidentified e_machine {machine:#06x}:\n{stdout}"
        );
    }

    let text = base.join("not-an-elf");
    fs::write(&text, "plain text, not an ELF file\n").expect("write the non-ELF fixture");
    let output = Command::new("bash")
        .arg(&helper)
        .arg(&text)
        .current_dir(repo_root())
        .output()
        .expect("spawn bash tools/elf-machine.sh on non-ELF input");
    assert!(
        !output.status.success(),
        "tools/elf-machine.sh accepted non-ELF input with status {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn deb_script_rejects_a_non_amd64_host() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-deb-host-arch"));
    let base = holder.path();
    let shim_dir = base.join("uname-shims");
    write_uname_shim(&shim_dir, "aarch64");

    let output = stage_deb_dry_run_with_path_prefix(base, Some(&shim_dir));
    let diagnostics = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "tools/vmodv-deb.sh accepted an aarch64 host and would build an amd64 .deb \
         (status {:?}):\n{diagnostics}",
        output.status,
    );
    assert_contains(
        &diagnostics,
        "amd64",
        "the non-amd64 host rejection must explain that .deb packages are amd64-only",
    );
    assert_contains(
        &diagnostics,
        "aarch64",
        "the non-amd64 host rejection must name the detected host architecture",
    );
}

#[test]
fn deb_script_rejects_a_non_x86_64_binary() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-deb-binary-arch"));
    let base = holder.path();
    let setup = stage_deb_dry_run(base);
    assert!(
        setup.status.success(),
        "could not stage the fake project before testing binary validation: {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        setup.status,
        String::from_utf8_lossy(&setup.stdout),
        String::from_utf8_lossy(&setup.stderr),
    );

    let binary = base.join("modv-aarch64");
    write_elf_header(&binary, 0x00b7); // EM_AARCH64
    let repo_root = repo_root();
    let output = Command::new("bash")
        .arg(repo_root.join("tools/vmodv-deb.sh"))
        .arg("--dry-run")
        .arg("--project-root")
        .arg(base.join("project"))
        .arg("--binary")
        .arg(&binary)
        .arg("--version")
        .arg("9.9.9")
        .env_remove("APP_VERSION")
        .env_remove("VMODV_VERSION")
        .current_dir(&repo_root)
        .output()
        .expect("spawn bash tools/vmodv-deb.sh --dry-run --binary <aarch64 ELF>");
    let diagnostics = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !output.status.success(),
        "tools/vmodv-deb.sh --dry-run accepted an AArch64 ELF for an amd64 package \
         (status {:?}):\n{diagnostics}",
        output.status,
    );
    assert_contains(
        &diagnostics,
        "aarch64",
        "the binary rejection must name the detected ELF machine",
    );
}

#[test]
fn deb_checker_validates_the_payload_elf_machine() {
    let checker = repo_root().join("tools/vmodv-deb-check.sh");
    assert!(
        checker.is_file(),
        "{} is missing: there is no .deb checker",
        checker.display()
    );
    let text = read(&checker);

    assert!(
        text.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#')
                && line.contains("elf-machine")
                && line.contains("usr/lib/vmodv/modv")
        }),
        "tools/vmodv-deb-check.sh must invoke tools/elf-machine.sh on the extracted \
         usr/lib/vmodv/modv payload:\n{text}"
    );
    assert!(
        text.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#')
                && line.contains("x86_64")
                && (line.contains('=') || line.starts_with("x86_64)"))
        }),
        "tools/vmodv-deb-check.sh must require the payload ELF machine to be x86_64:\n{text}"
    );
}

#[test]
fn build_target_skips_the_deb_on_non_amd64() {
    let holder = TempDirGuard::new(&unique_dir_name("vmodv-build-host-arch"));
    let base = holder.path();
    let shim_dir = base.join("uname-shims");
    write_uname_shim(&shim_dir, "aarch64");
    let repo_root = repo_root();

    let arm_output = Command::new("make")
        .args(["-n", "build"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                shim_dir.display(),
                std::env::var_os("PATH")
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
        )
        .current_dir(&repo_root)
        .output()
        .expect("spawn make -n build with the aarch64 uname shim");
    assert!(
        arm_output.status.success(),
        "make -n build failed with the aarch64 shim: {:?}\nstdout:\n{}\nstderr:\n{}",
        arm_output.status,
        String::from_utf8_lossy(&arm_output.stdout),
        String::from_utf8_lossy(&arm_output.stderr),
    );
    let arm_plan = String::from_utf8_lossy(&arm_output.stdout);
    assert_contains(
        &arm_plan,
        "tools/vmodv-appimage.sh",
        "the aarch64 make -n build plan (native AppImage)",
    );
    assert!(
        !arm_plan.contains("vmodv-deb"),
        "make -n build on aarch64 still includes the amd64-only vmodv-deb target:\n{arm_plan}"
    );

    // Run the same dry-run command without the shim so the AppImage and .deb
    // assertions cannot both pass against a broken or empty make plan.
    let native_output = Command::new("make")
        .args(["-n", "build"])
        .current_dir(&repo_root)
        .output()
        .expect("spawn make -n build with the real host architecture");
    assert!(
        native_output.status.success(),
        "make -n build failed on the native host: {:?}\nstdout:\n{}\nstderr:\n{}",
        native_output.status,
        String::from_utf8_lossy(&native_output.stdout),
        String::from_utf8_lossy(&native_output.stderr),
    );
    let native_plan = String::from_utf8_lossy(&native_output.stdout);
    assert_contains(
        &native_plan,
        "tools/vmodv-appimage.sh",
        "the native make -n build plan (AppImage)",
    );
    assert_contains(
        &native_plan,
        "vmodv-deb",
        "the native x86_64 make -n build plan must retain the .deb target",
    );
}
