//! RED guards for native x86_64 and arm64 AppImages in GitHub Actions.
//!
//! The packaging script and Makefile must derive appimagetool's architecture
//! token from the build host. Release and CI must use independent matrix legs
//! and architecture-scoped caches; a failed release leg must not discard the
//! other leg's image.
//!
//! Pure text/FS scanning: std only, no YAML crate, network or env mutation.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn read_repo_file(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The body of a top-level GitHub Actions job (a two-space-indented key).
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

fn needs_of(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim().strip_prefix("needs:") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(items) = rest.strip_prefix('[') {
            return items
                .trim_end_matches(']')
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
        }
        if rest.is_empty() {
            let mut needs = Vec::new();
            for follow in lines.iter().skip(index + 1) {
                let follow = follow.trim();
                if let Some(item) = follow.strip_prefix("- ") {
                    needs.push(item.trim().to_string());
                } else if follow.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            return needs;
        }
        return vec![rest.to_string()];
    }
    Vec::new()
}

/// The body of a Makefile target, including its tab-indented recipe.
fn make_recipe(text: &str, target: &str) -> String {
    let key = format!("{target}:");
    let with_prereqs = format!("{key} ");
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| {
        let line = line.trim_end();
        line == key || line.starts_with(&with_prereqs)
    }) else {
        return String::new();
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

/// One `matrix.include` entry beginning with `- arch: ARCH`.
fn matrix_leg(block: &str, arch: &str) -> Option<String> {
    let key = format!("- arch: {arch}");
    let lines: Vec<&str> = block.lines().collect();
    let start = lines.iter().position(|line| line.trim() == key)?;
    let mut end = lines.len();
    for (offset, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim_start().starts_with("- arch:") {
            end = offset;
            break;
        }
    }
    Some(lines[start..end].join("\n"))
}

/// Step bodies inside a job. This is deliberately indentation-based rather
/// than a YAML parser, matching the workflow's conventional block style.
fn job_steps(block: &str) -> Vec<String> {
    let lines: Vec<&str> = block.lines().collect();
    let Some(steps_start) = lines.iter().position(|line| line.trim() == "steps:") else {
        return Vec::new();
    };
    let mut steps: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in lines.into_iter().skip(steps_start + 1) {
        if line.trim_start().starts_with("- ") {
            if let Some(step) = current.take() {
                steps.push(step);
            }
            current = Some(line.to_string());
        } else if let Some(step) = current.as_mut() {
            step.push('\n');
            step.push_str(line);
        }
    }
    if let Some(step) = current {
        steps.push(step);
    }
    steps
}

fn has_arch_condition(step: &str, arch: &str) -> bool {
    let single = format!("'{arch}'");
    let double = format!("\"{arch}\"");
    step.lines().any(|line| {
        let line = line.trim();
        line.starts_with("if:")
            && line.contains("matrix.arch")
            && (line.contains(&single) || line.contains(&double))
    })
}

fn assert_cargo_cache_is_per_arch(block: &str, context: &str) {
    let cache = job_steps(block)
        .into_iter()
        .find(|step| {
            step.contains("uses: actions/cache@v4")
                && step.contains(".cargo-cache")
                && step.contains("crates/target")
        })
        .unwrap_or_else(|| panic!("{context} has no cargo registry/target cache step"));
    assert!(
        cache
            .lines()
            .any(|line| line.trim_start().starts_with("key:") && line.contains("matrix.arch")),
        "{context}'s cargo cache key is shared between architectures; include matrix.arch so \
         arm64 cannot restore x86_64 target artifacts:\n{cache}"
    );
    let restore_keys = cache
        .split_once("restore-keys:")
        .map(|(_, keys)| keys)
        .unwrap_or_default();
    assert!(
        restore_keys.contains("matrix.arch"),
        "{context}'s cargo restore-keys prefix is shared between architectures; include \
         matrix.arch in the fallback prefix too:\n{cache}"
    );
}

#[test]
fn appimage_script_selects_the_tool_and_output_for_the_host_architecture() {
    let script = read_repo_file("tools/vmodv-appimage.sh");

    for old in [
        "VmodV-$VERSION-x86_64.AppImage",
        "appimagetool-x86_64.AppImage",
        "ARCH=x86_64",
    ] {
        assert!(
            !script.contains(old),
            "tools/vmodv-appimage.sh still hardcodes {old:?}; neither executable code nor \
             header comments may retain the x86_64-only AppImage contract"
        );
    }

    assert!(
        script.contains("uname -m") && script.contains("APPIMAGE_ARCH"),
        "tools/vmodv-appimage.sh must resolve APPIMAGE_ARCH from `uname -m` instead of \
         assuming the host is x86_64"
    );
    let lines: Vec<&str> = script.lines().collect();
    let arch_start = lines
        .iter()
        .position(|line| line.contains("uname -m"))
        .expect("the AppImage script must inspect `uname -m`");
    let arch_end = lines[arch_start + 1..]
        .iter()
        .position(|line| line.trim() == "esac")
        .map(|offset| arch_start + 1 + offset + 1)
        .unwrap_or((arch_start + 20).min(lines.len()));
    let arch_setup = lines[arch_start..arch_end].join("\n");
    for token in ["x86_64", "amd64", "aarch64", "arm64"] {
        assert!(
            arch_setup.contains(token),
            "the APPIMAGE_ARCH resolver does not map {token}; expected x86_64/amd64 -> \
             x86_64 and aarch64/arm64 -> aarch64:\n{arch_setup}"
        );
    }
    assert!(
        arch_setup.contains("*)"),
        "the APPIMAGE_ARCH resolver has no unsupported-architecture case:\n{arch_setup}"
    );
    let readable_error = arch_setup.to_ascii_lowercase();
    assert!(
        readable_error.contains("unsupported")
            && readable_error.contains("architecture")
            && (readable_error.contains("fail ") || readable_error.contains("exit 1")),
        "unsupported host architectures must fail with a readable error naming the \
         architecture:\n{arch_setup}"
    );

    let image_forms = [
        "IMAGE=\"$OUT/VmodV-$VERSION-$APPIMAGE_ARCH.AppImage\"",
        "IMAGE=\"$OUT/VmodV-$VERSION-${APPIMAGE_ARCH}.AppImage\"",
    ];
    assert!(
        image_forms.iter().any(|form| script.contains(form)),
        "the AppImage output must be $OUT/VmodV-$VERSION-$APPIMAGE_ARCH.AppImage; the script \
         still does not name the host-architecture artifact"
    );
    let download_forms = [
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$APPIMAGE_ARCH.AppImage",
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${APPIMAGE_ARCH}.AppImage",
    ];
    assert!(
        download_forms.iter().any(|form| script.contains(form)),
        "the appimagetool download must select appimagetool-$APPIMAGE_ARCH.AppImage for the \
         native host instead of downloading the x86_64 tool on arm64"
    );

    let tool_assignments: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| {
            !line.trim_start().starts_with('#')
                && line.contains("appimagetool")
                && line.contains('=')
        })
        .collect();
    assert!(
        !tool_assignments.is_empty(),
        "the AppImage script has no cached appimagetool path assignment to scope by architecture"
    );
    for assignment in tool_assignments {
        assert!(
            assignment.contains("$APPIMAGE_ARCH") || assignment.contains("${APPIMAGE_ARCH}"),
            "the cached appimagetool path is shared between architectures; include \
             APPIMAGE_ARCH in its filename: {assignment}"
        );
    }
    assert!(
        script.contains("ARCH=\"$APPIMAGE_ARCH\"") || script.contains("ARCH=\"${APPIMAGE_ARCH}\""),
        "appimagetool must be invoked with ARCH set to the resolved APPIMAGE_ARCH, not a \
         hardcoded x86_64 value"
    );
}

#[test]
fn makefile_checks_the_arch_specific_appimage() {
    let makefile = read_repo_file("Makefile");
    let lines: Vec<&str> = makefile.lines().collect();
    let arch_start = lines
        .iter()
        .position(|line| line.contains("APPIMAGE_ARCH"))
        .expect("Makefile must define APPIMAGE_ARCH so its checker follows the native image");
    let arch_setup =
        lines[arch_start.saturating_sub(8)..(arch_start + 12).min(lines.len())].join("\n");
    assert!(
        arch_setup.contains("uname -m"),
        "Makefile must derive APPIMAGE_ARCH from `uname -m` so its checker follows the \
         native AppImage architecture:\n{arch_setup}"
    );
    for token in ["x86_64", "amd64", "aarch64", "arm64"] {
        assert!(
            arch_setup.contains(token),
            "Makefile's APPIMAGE_ARCH mapping is missing {token}; map amd64/x86_64 to \
             x86_64 and arm64/aarch64 to aarch64:\n{arch_setup}"
        );
    }
    assert!(
        !makefile.contains("VmodV-$(VMODV_VERSION)-x86_64.AppImage"),
        "Makefile still hardcodes VmodV-$(VMODV_VERSION)-x86_64.AppImage; the checker must \
         select the architecture-specific image"
    );
    let checker = make_recipe(&makefile, "vmodv-appimage-check");
    assert!(
        checker.contains(
            "bash /app/tools/vmodv-appimage-check.sh /app/$(DIST_DIR)/VmodV-$(VMODV_VERSION)-$(APPIMAGE_ARCH).AppImage"
        ),
        "vmodv-appimage-check must pass \
         /app/$(DIST_DIR)/VmodV-$(VMODV_VERSION)-$(APPIMAGE_ARCH).AppImage to the checker:\n{checker}"
    );
}

#[test]
fn release_workflow_builds_the_arm64_appimage_on_the_arm64_runner() {
    let release = read_repo_file(".github/workflows/release.yml");
    let build = job_block(&release, "build-appimage")
        .expect("release.yml has no `build-appimage` job for native Linux AppImages");

    assert!(
        build.contains("strategy:")
            && build.contains("fail-fast: false")
            && build.contains("matrix:"),
        "release.yml's build-appimage job must be a two-leg matrix with fail-fast disabled so \
         one architecture cannot cancel the other:\n{build}"
    );
    let leg_count = build
        .lines()
        .filter(|line| line.trim_start().starts_with("- arch:"))
        .count();
    assert_eq!(
        leg_count, 2,
        "release.yml's build-appimage matrix must contain exactly the x86_64 and aarch64 legs"
    );
    assert!(
        build.contains("runs-on: ${{ matrix.runner }}"),
        "release.yml's build-appimage matrix must select each leg's native runner via \
         runs-on: ${{ matrix.runner }}"
    );
    for (arch, runner, artifact) in [
        ("x86_64", "ubuntu-latest", "vmodv-appimage-x86_64"),
        ("aarch64", "ubuntu-24.04-arm", "vmodv-appimage-aarch64"),
    ] {
        let leg = matrix_leg(&build, arch)
            .unwrap_or_else(|| panic!("build-appimage has no {arch} matrix leg"));
        assert!(
            leg.contains(&format!("runner: {runner}")),
            "the {arch} AppImage leg must run natively on {runner}:\n{leg}"
        );
        assert!(
            leg.contains(&format!("artifact: {artifact}")),
            "the {arch} AppImage leg must upload the distinct artifact {artifact}:\n{leg}"
        );
    }
    assert!(
        build.contains("uses: actions/upload-artifact@v4")
            && build.contains("name: ${{ matrix.artifact }}")
            && build.contains("path: dist/VmodV-*.AppImage"),
        "build-appimage must upload each leg's dist/VmodV-*.AppImage as \
         ${{{{ matrix.artifact }}}}:\n{build}"
    );
    for command in ["make appimage-vmodv", "make vmodv-appimage-check"] {
        assert_eq!(
            release.matches(command).count(),
            1,
            "release.yml must contain `{command}` exactly once in the shared matrix step; \
             comments or duplicate legs must not repeat it"
        );
        assert!(
            job_steps(&build).iter().any(|step| step
                .lines()
                .any(|line| line.trim() == format!("run: {command}"))),
            "the shared build-appimage matrix steps must run `{command}` for both architectures"
        );
    }
    assert_cargo_cache_is_per_arch(&build, "release.yml build-appimage");
    let gha_cache_lines: Vec<&str> = build
        .lines()
        .filter(|line| line.contains("type=gha"))
        .collect();
    assert!(
        !gha_cache_lines.is_empty(),
        "build-appimage must cache the Docker Rust toolchain through GitHub Actions"
    );
    for line in gha_cache_lines {
        assert!(
            line.contains("scope=") && line.contains("matrix.arch"),
            "build-appimage's Docker toolchain GHA cache scope must be per-architecture; \
             include matrix.arch so arm64 never restores x86_64 image layers: {line}"
        );
    }
}

#[test]
fn release_attach_keeps_the_surviving_architectures_appimage() {
    let release = read_repo_file(".github/workflows/release.yml");
    let attach =
        job_block(&release, "attach-appimage").expect("release.yml has no `attach-appimage` job");

    let if_condition = attach
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("if:"))
        .map(str::trim)
        .expect("attach-appimage has no job-level `if:` guard");
    assert!(
        if_condition.contains("!cancelled()") || if_condition.contains("always()"),
        "attach-appimage still uses the default success gate; one failed matrix leg skips \
         the job and drops the surviving architecture's image (if: {if_condition})"
    );
    assert!(
        if_condition.contains("needs.release.result == 'success'"),
        "attach-appimage must still require a successful release before uploading assets \
         (if: {if_condition})"
    );
    assert!(
        !if_condition.contains("needs.build-appimage.result == 'success'"),
        "attach-appimage must not require the whole build-appimage matrix to succeed; one \
         failed leg must not discard the other image (if: {if_condition})"
    );

    let steps = job_steps(&attach);
    let download = steps
        .iter()
        .find(|step| {
            step.contains("uses: actions/download-artifact@v4")
                && step.contains("pattern: vmodv-appimage-*")
        })
        .unwrap_or_else(|| {
            panic!("attach-appimage must best-effort download all vmodv-appimage-* artifacts")
        });
    assert!(
        download.contains("continue-on-error: true"),
        "a missing AppImage architecture must not abort attachment of the surviving \
         artifact; mark the wildcard download continue-on-error: true:\n{download}"
    );
    let upload = steps
        .iter()
        .find(|step| step.contains("gh release upload"))
        .unwrap_or_else(|| panic!("attach-appimage has no `gh release upload` step"));
    assert!(
        upload.contains("*.AppImage"),
        "attach-appimage must upload the downloaded *.AppImage files:\n{upload}"
    );
    let upload_position = upload
        .find("gh release upload")
        .expect("the upload step contains gh release upload");
    let guarded_before_upload = upload[..upload_position].contains("nullglob")
        || upload[..upload_position].contains("compgen -G");
    assert!(
        guarded_before_upload,
        "attach-appimage must enable nullglob (or use compgen -G) before gh release upload; \
         otherwise an empty dist directory passes a literal *.AppImage pattern to gh:\n{upload}"
    );
    assert!(
        attach.contains("uses: actions/checkout@") || attach.contains("GH_REPO:"),
        "attach-appimage runs gh without a checkout or GH_REPO, so gh cannot resolve the \
         repository for the upload"
    );

    let release_job = job_block(&release, "release").expect("release.yml has no `release` job");
    assert_eq!(
        needs_of(&release_job),
        vec!["version".to_string()],
        "`release` must still need only `version`; AppImage matrix failures must never gate \
         tagging or publishing the release"
    );
}

#[test]
fn ci_package_workflow_builds_the_arm64_appimage() {
    let ci = read_repo_file(".github/workflows/vmodv.yml");
    let package = job_block(&ci, "package")
        .expect("vmodv.yml has no `package` job for native Linux installers");

    assert!(
        package
            .contains("if: ${{ github.event_name != 'push' || github.ref != 'refs/heads/main' }}"),
        "the package job must retain its main-push guard so release.yml remains the only \
         main-push installer build"
    );
    assert!(
        package.contains("strategy:")
            && package.contains("fail-fast: false")
            && package.contains("matrix:"),
        "vmodv.yml's package job must become a two-leg native matrix with fail-fast disabled"
    );
    let leg_count = package
        .lines()
        .filter(|line| line.trim_start().starts_with("- arch:"))
        .count();
    assert_eq!(
        leg_count, 2,
        "vmodv.yml's package matrix must contain exactly the x86_64 and aarch64 legs"
    );
    assert!(
        package.contains("runs-on: ${{ matrix.runner }}"),
        "the package matrix must use each leg's native runner via \
         runs-on: ${{ matrix.runner }}"
    );
    for (arch, runner) in [("x86_64", "ubuntu-latest"), ("aarch64", "ubuntu-24.04-arm")] {
        let leg = matrix_leg(&package, arch)
            .unwrap_or_else(|| panic!("vmodv.yml's package job has no {arch} matrix leg"));
        assert!(
            leg.contains(&format!("runner: {runner}")),
            "the {arch} CI package leg must run on {runner}:\n{leg}"
        );
    }

    let steps = job_steps(&package);
    let x64_build = steps
        .iter()
        .find(|step| step.lines().any(|line| line.trim() == "run: make build"))
        .expect("the x86_64 package leg must still run `make build` for the AppImage and .deb");
    assert!(
        has_arch_condition(x64_build, "x86_64"),
        "`make build` (which creates the amd64-only .deb) must be gated to the x86_64 leg:\n{x64_build}"
    );
    let deb_inspection = steps
        .iter()
        .find(|step| step.contains("dpkg-deb -I"))
        .expect("the x86_64 package leg must inspect the .deb with dpkg-deb -I");
    assert!(
        has_arch_condition(deb_inspection, "x86_64") && deb_inspection.contains("dpkg-deb -c"),
        ".deb inspection must stay on the x86_64 leg and retain both dpkg-deb -I and \
         dpkg-deb -c:\n{deb_inspection}"
    );
    let arm_build = steps
        .iter()
        .find(|step| {
            step.lines()
                .any(|line| line.trim() == "run: make appimage-vmodv")
        })
        .expect("the aarch64 package leg must run `make appimage-vmodv` natively");
    assert!(
        has_arch_condition(arm_build, "aarch64"),
        "the AppImage-only arm64 build must be gated to the aarch64 matrix leg:\n{arm_build}"
    );
    assert!(
        steps.iter().any(|step| {
            step.lines()
                .any(|line| line.trim() == "run: make vmodv-appimage-check")
        }),
        "both package matrix legs must run `make vmodv-appimage-check` on their native image"
    );

    let uploads: Vec<&String> = steps
        .iter()
        .filter(|step| step.contains("uses: actions/upload-artifact@v4"))
        .collect();
    assert!(
        !uploads.is_empty(),
        "the package matrix has no actions/upload-artifact step"
    );
    let mut found_x64_upload = false;
    let mut found_arm_upload = false;
    for upload in uploads {
        if has_arch_condition(upload, "x86_64") || upload.contains("name: vmodv-linux-x64") {
            found_x64_upload = true;
            assert!(
                upload.contains("name: vmodv-linux-x64")
                    && upload.contains("dist/VmodV-*.AppImage")
                    && upload.contains("dist/VmodV-*.deb"),
                "the x86_64 artifact upload must be named vmodv-linux-x64 and include both \
                 the AppImage and .deb:\n{upload}"
            );
        }
        if has_arch_condition(upload, "aarch64") || upload.contains("name: vmodv-linux-arm64") {
            found_arm_upload = true;
            assert!(
                upload.contains("name: vmodv-linux-arm64")
                    && upload.contains("dist/VmodV-*.AppImage")
                    && !upload.contains(".deb"),
                "the arm64 artifact upload must be named vmodv-linux-arm64 and contain only \
                 the AppImage, never the amd64-only .deb:\n{upload}"
            );
        }
        if upload.contains("${{ matrix.artifact }}") {
            let x64_leg = matrix_leg(&package, "x86_64").expect("checked x86_64 matrix leg above");
            let arm_leg = matrix_leg(&package, "aarch64").expect("checked arm64 matrix leg above");
            assert!(
                x64_leg.contains("artifact: vmodv-linux-x64")
                    && arm_leg.contains("artifact: vmodv-linux-arm64"),
                "a shared matrix upload must map x86_64 to vmodv-linux-x64 and aarch64 to \
                 vmodv-linux-arm64"
            );
            assert!(
                upload.contains("dist/VmodV-*.AppImage"),
                "the shared matrix upload must include dist/VmodV-*.AppImage for both legs"
            );
            assert!(
                x64_leg.contains(".deb")
                    || upload.lines().any(|line| {
                        line.contains(".deb")
                            && line.contains("matrix.arch")
                            && line.contains("x86_64")
                    }),
                "the x86_64 matrix leg must upload its .deb as well as the shared AppImage"
            );
            assert!(
                !arm_leg.contains(".deb"),
                "the arm64 matrix leg must remain AppImage-only and must not configure a \
                 .deb upload path"
            );
            for line in upload.lines().filter(|line| line.contains(".deb")) {
                assert!(
                    line.contains("matrix.arch") && line.contains("x86_64"),
                    "a shared upload leaves the .deb available to arm64; guard every .deb \
                     path with the x86_64 matrix condition: {line}"
                );
            }
            found_x64_upload = true;
            found_arm_upload = true;
        }
    }
    assert!(
        found_x64_upload,
        "the package matrix never uploads vmodv-linux-x64 with both AppImage and .deb"
    );
    assert!(
        found_arm_upload,
        "the package matrix never uploads vmodv-linux-arm64 with only the AppImage"
    );
}

#[test]
fn ci_workflows_scope_caches_per_architecture() {
    let release = read_repo_file(".github/workflows/release.yml");
    let ci = read_repo_file(".github/workflows/vmodv.yml");
    let release_build = job_block(&release, "build-appimage")
        .expect("release.yml has no build-appimage job whose caches must be arch-scoped");
    let ci_package = job_block(&ci, "package")
        .expect("vmodv.yml has no package job whose caches must be arch-scoped");

    assert_cargo_cache_is_per_arch(&release_build, "release.yml build-appimage");
    assert_cargo_cache_is_per_arch(&ci_package, "vmodv.yml package");

    let release_gha_caches: Vec<&str> = release_build
        .lines()
        .filter(|line| line.contains("type=gha"))
        .collect();
    assert!(
        !release_gha_caches.is_empty(),
        "release.yml build-appimage must retain its Docker toolchain GHA cache"
    );
    for line in release_gha_caches {
        assert!(
            line.contains("scope=") && line.contains("matrix.arch"),
            "release.yml's Docker toolchain GHA cache scope must contain matrix.arch; an \
             architecture-neutral scope can restore x86_64 layers into the arm64 build: {line}"
        );
    }
}
