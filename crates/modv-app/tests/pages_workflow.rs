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
