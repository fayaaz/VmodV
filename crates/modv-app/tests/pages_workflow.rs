//! PAGES-01: the browser shell is deployed from `main` through GitHub Actions.
//!
//! The repository's Pages source is "GitHub Actions", not the `gh-pages`
//! branch. The workflow therefore has to upload and deploy a Pages artifact;
//! a workflow that only pushes `gh-pages` builds the shell and publishes
//! nothing, leaving the site on the last manually pushed branch commit.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test pages_workflow -- --test-threads=1"
//! ```

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

fn pages_workflow() -> String {
    let path = repo_root().join(".github/workflows/pages.yml");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn pages_workflow_builds_the_browser_shell_from_main() {
    let workflow = pages_workflow();

    assert!(
        workflow.contains("branches:\n      - main"),
        "PAGES-01: pages.yml must trigger on pushes to main; it was:\n{workflow}"
    );
    assert!(
        workflow.contains("workflow_dispatch:"),
        "PAGES-01: pages.yml must remain manually dispatchable; it was:\n{workflow}"
    );
    assert!(
        workflow.contains("if: github.ref == 'refs/heads/main'"),
        "PAGES-01: a manual dispatch from another branch must not publish; it was:\n{workflow}"
    );
    assert!(
        workflow.contains("make web-build"),
        "PAGES-01: pages.yml must build the real browser shell; it was:\n{workflow}"
    );
}

#[test]
fn pages_workflow_runs_the_wasm_compile_gate() {
    let workflow = pages_workflow();
    let check = workflow
        .find("make web-check")
        .expect("PAGES-01: pages.yml must invoke `make web-check`, the wasm32 compile gate");
    let build = workflow
        .find("make web-build")
        .expect("PAGES-01: pages.yml must invoke `make web-build`");
    assert!(
        check < build,
        "PAGES-01: `make web-check` must run before `make web-build` so a \
         wasm32 compile failure never deploys a stale browser shell"
    );
}

/// Text of the self-contained download page (HTML, styles and script).
fn download_page() -> String {
    let dir = repo_root().join("web/download");
    let mut text = String::new();
    // Missing files yield an empty string so the tests below fail with an
    // assertion about the missing content rather than a file-read panic.
    for name in ["index.html", "style.css", "app.js"] {
        text.push_str(&fs::read_to_string(dir.join(name)).unwrap_or_default());
    }
    text
}

/// Read the hex colour of a named palette constant from the app's theme source.
fn theme_hex(constant: &str) -> String {
    let theme = fs::read_to_string(repo_root().join("crates/modv-ui/src/theme.rs"))
        .expect("cannot read crates/modv-ui/src/theme.rs");
    let marker = format!("pub const {constant}: Color32 = Color32::from_rgb(");
    let start = theme
        .find(&marker)
        .unwrap_or_else(|| panic!("no from_rgb definition for {constant} in theme.rs"));
    let args = &theme[start + marker.len()..];
    let end = args.find(')').expect("unterminated from_rgb call");
    let rgb = args[..end]
        .split(',')
        .map(|part| part.trim().parse::<u8>().expect("RGB channel"))
        .collect::<Vec<_>>();
    assert_eq!(rgb.len(), 3, "{constant} must have three RGB channels");
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

#[test]
fn download_page_is_staged_by_the_pages_workflow() {
    let page = repo_root().join("web/download/index.html");
    assert!(
        page.is_file(),
        "PAGES-02: the download landing page must live at web/download/index.html \
         so `cp -R web/. _site/` deploys it at /download/; {page:?} is missing"
    );
    assert!(
        pages_workflow().contains("cp -R web/. _site/"),
        "PAGES-02: pages.yml must copy all of web/ so /download/ ships with the shell"
    );
}

#[test]
fn download_page_links_the_latest_release_for_every_platform() {
    let page = download_page();
    for needle in [
        // Always resolve to whatever the latest published release is.
        "api.github.com/repos/fayaaz/VmodV/releases/latest",
        "github.com/fayaaz/VmodV/releases/latest",
        // Linux offers both formats and both AppImage architectures.
        "-x86_64.AppImage",
        "-aarch64.AppImage",
        "-amd64",
        "-ubuntu24.04.deb",
        "-ubuntu26.04.deb",
        "-ubuntu26.10.deb",
        "-ubuntudevel.deb",
        // Best-effort portable builds for the other platforms.
        "-Windows-x86_64-portable-",
        "-macOS-arm64-portable-",
        "-macOS-x86_64-portable-",
        // The Linux choice is an explicit AppImage/.deb switch.
        "id=\"tab-appimage\"",
        "id=\"tab-deb\"",
    ] {
        assert!(
            page.contains(needle),
            "PAGES-02: the download page must reference {needle:?} to link the \
             latest release asset for its platform"
        );
    }
}

#[test]
fn download_page_lede_does_not_repeat_the_platform_cards() {
    let page = download_page()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !page.contains(
            "Grab an AppImage or a Debian package below, or pick up a best-effort \
             portable build for Windows and macOS."
        ),
        "PAGES-02: the hero lede must stay short; the platform cards already \
         describe the available builds"
    );
}

#[test]
fn download_page_does_not_use_platform_taglines() {
    let page = download_page();
    assert!(
        !page.contains("class=\"card-sub\""),
        "PAGES-02: the platform cards must not carry taglines under their headings"
    );
    for tagline in [
        "Full feature set — the primary target.",
        "Best-effort portable ZIP — no installer.",
        "Best-effort portable app bundle — no installer.",
    ] {
        assert!(
            !page.contains(tagline),
            "PAGES-02: remove the platform tagline {tagline:?}"
        );
    }
}

#[test]
fn download_page_has_platform_specific_requirements() {
    // Collapse HTML wrapping so a sentence split across lines still matches.
    let page = download_page()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // One "What you need" block per download choice, not a single generic
    // paragraph that pretends every format has the same prerequisites.
    for id in [
        "needs-linux-appimage",
        "needs-linux-deb",
        "needs-windows",
        "needs-macos",
    ] {
        assert!(
            page.contains(&format!("id=\"{id}\"")),
            "PAGES-02: the download page must have a per-platform requirements block ({id})"
        );
    }
    assert!(
        !page.contains("Either way you need a Vulkan driver"),
        "PAGES-02: the one-size-fits-all requirements paragraph must be gone"
    );
    assert!(
        page.contains("installs the runtime dependencies"),
        "PAGES-02: the .deb card must say installing the package pulls in its dependencies"
    );
    assert!(
        page.contains("libfuse2"),
        "PAGES-02: the AppImage card must cover FUSE separately from the .deb card"
    );
}

#[test]
fn download_page_does_not_show_an_asset_count_status_line() {
    let page = download_page();
    assert!(
        !page.contains("attached to this release."),
        "PAGES-02: the download page must not render an asset-count status line; \
         the buttons already speak for themselves"
    );
    assert!(
        !page.contains("Buttons link straight to the asset files"),
        "PAGES-02: remove the redundant status sentence after the assets load"
    );
}

#[test]
fn download_page_uses_the_app_ui_palette() {
    let page = download_page().to_lowercase();
    // These are read from the app's theme.rs, so a palette change in the UI
    // fails this guard until the landing page is re-themed to match.
    for constant in [
        "BACKGROUND",
        "PANEL_ALT",
        "INPUT",
        "FOREGROUND",
        "MUTED",
        "ACCENT",
        "ACCENT_DIM",
        "BORDER",
    ] {
        let hex = theme_hex(constant);
        assert!(
            page.contains(&hex),
            "PAGES-02: the download page must use the app's {constant} colour ({hex})"
        );
    }
    assert!(
        page.contains("ibm plex sans") && page.contains("ibm plex mono"),
        "PAGES-02: the page must use the same IBM Plex family stack as the app UI"
    );
    assert!(
        page.contains("border-radius: 0"),
        "PAGES-02: the app UI uses square chrome, so the landing page must too"
    );
}

#[test]
fn pages_workflow_deploys_an_artifact_instead_of_pushing_gh_pages() {
    let workflow = pages_workflow();

    for action in [
        "actions/configure-pages@",
        "actions/upload-pages-artifact@",
        "actions/deploy-pages@",
    ] {
        assert!(
            workflow.contains(action),
            "PAGES-01: pages.yml must use {action} for an Actions Pages deployment; \
             it was:\n{workflow}"
        );
    }

    for permission in ["pages: write", "id-token: write", "contents: read"] {
        assert!(
            workflow.contains(permission),
            "PAGES-01: pages.yml must grant `{permission}`; it was:\n{workflow}"
        );
    }

    assert!(
        workflow.contains("name: github-pages"),
        "PAGES-01: the deploy job must target the github-pages environment; it was:\n{workflow}"
    );
    assert!(
        workflow.contains("path: _site"),
        "PAGES-01: pages.yml must upload the staged site as the Pages artifact; \
         it was:\n{workflow}"
    );
    assert!(
        workflow.contains("cp -R web/. _site/"),
        "PAGES-01: the staged site must contain the built browser shell; it was:\n{workflow}"
    );

    assert!(
        !workflow.contains("gh-pages"),
        "PAGES-01: Pages deploys from Actions now, so pages.yml must not publish \
         a gh-pages branch; it was:\n{workflow}"
    );
    assert!(
        !workflow.contains("git push"),
        "PAGES-01: pages.yml must deploy through actions/deploy-pages, not by \
         pushing a branch; it was:\n{workflow}"
    );
}
