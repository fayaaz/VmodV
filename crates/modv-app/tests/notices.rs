//! Guard the third-party notices against the Rust-only tree.
//!
//! This is the Rust replacement for `tests/unit/third-party-notices.test.js`.
//! It keeps the two properties that matter after the Electron tree is gone:
//!
//! * every registry crate in `crates/Cargo.lock` has a row with a valid SPDX
//!   licence and a plausible upstream link, and every row still resolves;
//! * every in-tree file that carries an NC-SA marker is named in
//!   `THIRD_PARTY_NOTICES.md`, and its author is credited. The scan covers the
//!   Rust ports under `crates/`, the ISF corpus, the bundled Point10 assets
//!   and `.mlk` files. The only `.mlk` in this tree is the guard fixture under
//!   `tests/fixtures/notices`: user MilkDrop presets are read in place from
//!   the user's media folders and are never bundled, so they are not scanned
//!   here. ISF/Point10 sources name their author in a `"CREDIT"` field, while
//!   MilkDrop presets use a leading `// Author:` comment line.
//!
//! It also pins the specific attribution corrections the JS guard used to
//! cover (st_Ms2SD1, spherical-shader-tut, Perlin Transition, Helix Eye,
//! Traced Tunnel and Around Plus), the Point10 scenes' provenance table, and
//! the README licence badge, so those do not regress when the notices are edited.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test notices"
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use modv_app::module_docs::{build_document, split_documents};
use modv_app::module_registry::{ModuleKind, ModuleRegistry};
use modv_app::store_ops::Store;
use modv_render::{Engine, EngineConfig, wgpu};
use modv_renderers::point10::audio_bridge::{AUDIO_SAMPLES, AudioBridge, BandPayload};
use modv_renderers::point10::postfx::Composer;
use modv_renderers::point10::{HDR_FORMAT, Point10Scene, SceneContext, all_scenes};
use modv_ui::model::{ModuleMetaView, module_license};
use serde_json::{Value, json};

const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");
const CARGO_LOCK: &str = include_str!("../../Cargo.lock");
const README: &str = include_str!("../../../README.md");

const NOTICES_SECTION: &str = "Rust engine dependencies";

const WORKSPACE_CRATES: &[&str] = &[
    "modv-app",
    "modv-engine",
    "modv-ipc",
    "modv-media",
    "modv-preview-wasm",
    "modv-render",
    "modv-renderers",
    "modv-ui",
    "modv-web",
];

/// SPDX identifiers used by the workspace's dependency graph (plus the common
/// permissive ids, so a new crate does not need a list edit unless it adds an
/// identifier nobody has used yet).
const SPDX_IDENTIFIERS: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC-BY-3.0",
    "CC-BY-4.0",
    "CC0-1.0",
    "GPL-2.0-only",
    "ISC",
    "LGPL-2.1-or-later",
    "LLVM-exception",
    "MIT",
    "MPL-2.0",
    "NCSA",
    "OFL-1.1",
    "Ubuntu-font-1.0",
    "Unicode-DSL-1.0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
];

/// Hosts accepted as plausible upstream links.
const KNOWN_FORGES: &[&str] = &[
    "github.com",
    "gitlab.com",
    "freedesktop.org",
    "redox-os.org",
    "codeberg.org",
    "crates.io",
    "docs.rs",
    "bitbucket.org",
    "sourceforge.net",
];

#[derive(Debug)]
struct LockPackage {
    name: String,
    source: Option<String>,
}

#[derive(Debug)]
struct NoticeRow {
    licence: String,
    url: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves")
}

fn toml_string(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

/// Parse the `[[package]]` entries from `crates/Cargo.lock`. Entries without a
/// `source` are the workspace's own crates (first-party), everything else
/// comes from a registry and therefore needs a notice row.
fn cargo_lock_packages(lock: &str) -> Vec<LockPackage> {
    let mut packages = Vec::new();
    let mut current: Option<LockPackage> = None;

    for line in lock.lines() {
        if line == "[[package]]" {
            current = Some(LockPackage {
                name: String::new(),
                source: None,
            });
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if let Some(value) = line.strip_prefix("name = ") {
            package.name = toml_string(value);
        } else if let Some(value) = line.strip_prefix("source = ") {
            package.source = Some(toml_string(value));
        } else if line.is_empty() {
            if !package.name.is_empty() {
                packages.push(current.take().expect("a package is in progress"));
            } else {
                current = None;
            }
        }
    }
    if let Some(package) = current
        && !package.name.is_empty()
    {
        packages.push(package);
    }

    packages
}

fn notices_section(heading: &str) -> &'static str {
    let marker = format!("## {heading}");
    let start = NOTICES
        .find(marker.as_str())
        .unwrap_or_else(|| panic!("notices must have a \"{marker}\" section"));
    let after_start = start + marker.len();
    let end = NOTICES[after_start..]
        .find("\n## ")
        .map(|offset| after_start + offset)
        .unwrap_or(NOTICES.len());
    &NOTICES[start..end]
}

/// One row per registry crate: `| `name` | SPDX licence | upstream link |`.
fn rust_dependency_rows(section: &str) -> Vec<(String, NoticeRow)> {
    let mut rows = Vec::new();

    for line in section.lines() {
        if !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() != 5 {
            continue;
        }
        let Some(name) = cells[1]
            .strip_prefix('`')
            .and_then(|name| name.strip_suffix('`'))
        else {
            continue;
        };
        let url = cells[3];
        if !url.starts_with("https://") {
            continue;
        }
        rows.push((
            name.to_string(),
            NoticeRow {
                licence: cells[2].to_string(),
                url: url.to_string(),
            },
        ));
    }

    rows
}

/// Validate an SPDX licence expression: identifiers joined by `AND`/`OR`, with
/// optional parentheses and `WITH` exceptions. Anything else (free text,
/// shorthand like `MIT/Apache-2.0`) fails.
fn is_spdx_expression(expression: &str) -> bool {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in expression.chars() {
        match character {
            '(' | ')' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(character.to_string());
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut expect_atom = true;
    let mut depth = 0usize;
    for token in tokens {
        match token.as_str() {
            "(" => {
                if !expect_atom {
                    return false;
                }
                depth += 1;
            }
            ")" => {
                if expect_atom || depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            "AND" | "OR" => {
                if expect_atom {
                    return false;
                }
                expect_atom = true;
            }
            "WITH" => {
                // `A WITH <exception>`: an exception identifier must follow.
                if expect_atom {
                    return false;
                }
                expect_atom = true;
            }
            identifier => {
                if !expect_atom || !SPDX_IDENTIFIERS.contains(&identifier) {
                    return false;
                }
                expect_atom = false;
            }
        }
    }

    !expect_atom && depth == 0
}

fn plausible_upstream(name: &str, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, _path)) = rest.split_once('/') else {
        return false;
    };
    let known_forge = KNOWN_FORGES
        .iter()
        .any(|forge| host == *forge || host.ends_with(&format!(".{forge}")));
    known_forge || url.contains(name)
}

/// Licence ids that carry obligations beyond "keep the notice": copyleft or
/// attribution terms that the notices must spell out.
fn obligation_licences(rows: &[(String, NoticeRow)]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (_, row) in rows {
        for word in row.licence.split(|character: char| {
            character.is_whitespace() || character == '(' || character == ')'
        }) {
            if matches!(word, "MPL-2.0" | "NCSA" | "OFL-1.1" | "Ubuntu-font-1.0")
                || word
                    .strip_prefix("CC-BY-")
                    .and_then(|rest| rest.chars().next())
                    .is_some_and(|character| character.is_ascii_digit())
            {
                found.insert(word.to_string());
            }
        }
    }
    found
}

fn obligation_bullets(section: &str) -> Vec<String> {
    let mut bullets: Vec<String> = Vec::new();
    for line in section.lines() {
        let line = line.trim();
        if line.starts_with("- ") {
            bullets.push(line.to_string());
        } else if !line.is_empty()
            && !line.starts_with('#')
            && !line.starts_with('|')
            && !line.starts_with('-')
            && let Some(bullet) = bullets.last_mut()
        {
            bullet.push(' ');
            bullet.push_str(line);
        }
    }
    bullets
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recursively collect `(relative path, source)` for text files that carry an
/// NC-SA marker. The text-extension allowlist includes `.mlk` so MilkDrop
/// preset fixtures are covered alongside ISF/Point10 sources; this guard file
/// itself is skipped so its marker strings do not make the scan self-match.
fn scan_source_files(dir: &Path, root: &Path, found: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() == "target" || entry.file_name() == ".git" {
                continue;
            }
            scan_source_files(&path, root, found);
            continue;
        }
        // These packaging guards name the markers they search for in their
        // doc comments and fixtures; none of them is an NC-SA source.
        if [
            "crates/modv-app/tests/notices.rs",
            "crates/modv-app/tests/appimage_bundle.rs",
            "crates/modv-app/tests/deb_package.rs",
            "crates/modv-renderers/src/point10/mod.rs",
        ]
        .iter()
        .any(|guard| path.ends_with(guard))
        {
            continue;
        }
        let text_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "rs" | "wgsl" | "fs" | "vs" | "frag" | "json" | "md" | "txt" | "svg" | "mlk"
                )
            });
        if !text_extension {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let lower = source.to_ascii_lowercase();
        if lower.contains("non-commercial")
            || lower.contains("noncommercial")
            || lower.contains("by-nc-sa")
        {
            found.push((relative(root, &path), source));
        }
    }
}

/// The in-tree roots the NC-SA guard scans. `resources/milkdrop` is absent in
/// this tree (no presets are bundled) and is a no-op; the `.mlk` guard fixture
/// lives under `crates/`, which is scanned. See the module docs for the
/// bundled-vs-user-supplied distinction.
fn nc_sa_sources(root: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for directory in [
        "crates",
        "resources/isf",
        "resources/point10",
        "resources/milkdrop",
    ] {
        scan_source_files(&root.join(directory), root, &mut found);
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// The author named in an ISF `"CREDIT"` field. Ports name the upstream author
/// after "by", followed by the source URL.
fn credit(source: &str) -> Option<String> {
    let marker = source.find("\"CREDIT\"")?;
    let after_marker = &source[marker + "\"CREDIT\"".len()..];
    let colon = after_marker.find(':')?;
    let after_colon = after_marker[colon + 1..].trim_start();
    let mut characters = after_colon.strip_prefix('"')?.chars();
    let mut raw = String::new();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                if let Some(escaped) = characters.next() {
                    raw.push(escaped);
                }
            }
            '"' => break,
            character => raw.push(character),
        }
    }
    let raw = raw.trim();

    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("port of ")
        && let Some(index) = lower.find(" by ")
    {
        let after_by = &raw[index + " by ".len()..];
        let end = after_by.find('(').unwrap_or(after_by.len());
        return Some(after_by[..end].trim().to_string());
    }

    let raw = raw
        .strip_prefix("by ")
        .or_else(|| raw.strip_prefix("By "))
        .unwrap_or(raw);
    Some(raw.trim().to_string())
}

/// The author credited in a MilkDrop `.mlk` preset's comment header.
///
/// MilkDrop presets do not carry ISF-style `"CREDIT"` JSON: the author is
/// written in a leading `//` comment line such as `// Author: Ada Lovelace`.
fn credit_milkdrop(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let value = line.trim().strip_prefix("//")?.trim_start();
        let author = value.strip_prefix("Author:")?.trim();
        (!author.is_empty()).then(|| author.to_string())
    })
}

#[test]
fn cargo_lock_scanner_finds_the_registry_and_workspace_packages() {
    let packages = cargo_lock_packages(CARGO_LOCK);
    let workspace: Vec<&str> = packages
        .iter()
        .filter(|package| package.source.is_none())
        .map(|package| package.name.as_str())
        .collect();
    let dependencies: BTreeSet<&str> = packages
        .iter()
        .filter(|package| package.source.is_some())
        .map(|package| package.name.as_str())
        .collect();

    assert_eq!(workspace, WORKSPACE_CRATES, "workspace crates");
    assert!(
        dependencies.len() > 500,
        "Cargo.lock scanner found only {} registry packages",
        dependencies.len()
    );
    for crate_name in ["wgpu", "winit", "naga", "tokio"] {
        assert!(
            dependencies.contains(crate_name),
            "{crate_name} is missing from crates/Cargo.lock"
        );
    }
}

#[test]
fn every_cargo_lock_registry_package_has_a_notice_row() {
    let packages = cargo_lock_packages(CARGO_LOCK);
    let rows = rust_dependency_rows(notices_section(NOTICES_SECTION));
    let row_names: BTreeSet<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    let dependencies: BTreeSet<&str> = packages
        .iter()
        .filter(|package| package.source.is_some())
        .map(|package| package.name.as_str())
        .collect();

    for dependency in &dependencies {
        assert!(
            row_names.contains(dependency),
            "{dependency} is in crates/Cargo.lock but has no row in the \
             \"{NOTICES_SECTION}\" table"
        );
    }

    let stale: Vec<&str> = row_names
        .iter()
        .copied()
        .filter(|name| !dependencies.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "rows without a Cargo.lock package (stale entries?): {stale:?}"
    );
    assert!(
        rows.len() > 500,
        "notice table has only {} rows",
        rows.len()
    );
}

#[test]
fn notice_rows_are_unique_spdx_and_link_upstream() {
    let rows = rust_dependency_rows(notices_section(NOTICES_SECTION));
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();

    let unique: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len(), "duplicate Rust notice rows");

    for (name, row) in &rows {
        assert!(
            is_spdx_expression(&row.licence),
            "{name} licence \"{}\" is not a valid SPDX expression",
            row.licence
        );
        assert!(
            plausible_upstream(name, &row.url),
            "{name} upstream \"{}\" is neither a known forge nor a link \
             containing the crate name",
            row.url
        );
    }
}

#[test]
fn copyleft_and_attribution_licences_have_an_obligation_bullet() {
    let section = notices_section(NOTICES_SECTION);
    let rows = rust_dependency_rows(section);
    let obligations = obligation_licences(&rows);
    assert!(
        !obligations.is_empty(),
        "the obligation matcher found nothing — coloured (MPL-2.0) should be in the graph"
    );

    let bullets = obligation_bullets(section);
    for licence in &obligations {
        assert!(
            bullets.iter().any(|bullet| bullet.contains(licence)),
            "{licence} crates need an obligation note"
        );
    }
    for bullet in &bullets {
        let names = rows
            .iter()
            .filter(|(name, _)| bullet.contains(&format!("`{name}`")));
        assert!(
            names.count() > 0,
            "obligation bullet \"{}…\" names no crate from the table",
            bullet.chars().take(60).collect::<String>()
        );
    }
}

#[test]
fn nc_sa_sources_are_scanned_and_named_in_the_notices() {
    let root = repo_root();
    let sources = nc_sa_sources(&root);
    let paths: Vec<&str> = sources.iter().map(|(path, _)| path.as_str()).collect();

    // Guards the scanner itself: if the header wording changes, the coverage
    // assertions below would otherwise pass vacuously.
    assert!(
        sources.len() >= 20,
        "the NC-SA scan found only {} files",
        sources.len()
    );
    // The packaging tests describe the markers they check for in the notices
    // payload; they are not themselves NC-SA-licensed sources. `point10/mod.rs`
    // is the registry contract: its Spiky test names the required notices row.
    for described in [
        "crates/modv-app/tests/appimage_bundle.rs",
        "crates/modv-app/tests/deb_package.rs",
        "crates/modv-renderers/src/point10/mod.rs",
    ] {
        assert!(
            !paths.contains(&described),
            "{described} only describes the markers; it must not be scanned as an NC-SA source"
        );
    }
    for expected in [
        "resources/isf/UltimateFlame.fs",
        "crates/modv-renderers/src/point10/scenes/boxy.rs",
        "crates/modv-renderers/src/point10/scenes/mirror_cube.rs",
    ] {
        assert!(paths.contains(&expected), "{expected} was not detected");
    }

    for (path, source) in &sources {
        assert!(
            NOTICES.contains(path),
            "{path} carries an NC-SA marker but is not listed in THIRD_PARTY_NOTICES.md"
        );
        if let Some(author) = credit(source) {
            assert!(
                NOTICES.contains(&author),
                "{path} names its author ({author}) but the notices do not"
            );
        }
    }
}

#[test]
fn milkdrop_ncsa_fixture_is_detected_and_named_in_the_notices() {
    // A `.mlk` fixture carrying an NC-SA marker must be picked up by the same
    // scan_source_files machinery that guards the bundled ISF/Point10 sources.
    // The fixture lives under tests/fixtures so no preset is bundled.
    let root = repo_root();
    let fixture_dir = root.join("crates/modv-app/tests/fixtures/notices");
    let mut found = Vec::new();
    scan_source_files(&fixture_dir, &root, &mut found);
    let paths: Vec<&str> = found.iter().map(|(path, _)| path.as_str()).collect();

    let fixture_path = "crates/modv-app/tests/fixtures/notices/milkdrop_ncsa.mlk";
    assert!(
        paths.contains(&fixture_path),
        "the NC-SA scanner ignored the MilkDrop .mlk fixture ({fixture_path}); \
         the text-extension list must include \"mlk\". found: {paths:?}"
    );

    assert!(
        NOTICES.contains(fixture_path),
        "{fixture_path} carries an NC-SA marker but is not named in \
         THIRD_PARTY_NOTICES.md"
    );
}

#[test]
fn milkdrop_presets_guide_is_linked_and_has_required_sections() {
    const DOC: &str = "docs/milkdrop-presets.md";

    assert!(
        README.contains(DOC),
        "the README licence note must link {DOC}"
    );
    assert!(
        NOTICES.contains(DOC),
        "THIRD_PARTY_NOTICES.md must link {DOC}"
    );

    let path = repo_root().join(DOC);
    let content = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{DOC} must exist with the MilkDrop preset guidance: {error}")
    });

    for heading in [
        "## What are MilkDrop presets",
        "## Where to put MilkDrop files",
        "## User files are read in place, never redistributed",
        "## Why no presets are bundled",
        "## Your responsibilities",
        "## Troubleshooting",
    ] {
        assert!(
            content.contains(heading),
            "{DOC} is missing the section {heading:?}"
        );
    }

    for fact in [
        ".mlk",
        "[preset00]",
        "MODV_USER_MILKDROP_DIR",
        "500 ms",
        "THIRD_PARTY_NOTICES.md",
    ] {
        assert!(content.contains(fact), "{DOC} must mention {fact:?}");
    }
}

#[test]
fn notices_header_distinguishes_bundled_and_user_supplied_presets() {
    const LINE: &str = "MilkDrop `.mlk` presets are not bundled: user-supplied presets are read in place and never redistributed by VmodV.";
    assert!(
        NOTICES.contains(LINE),
        "the THIRD_PARTY_NOTICES.md header must distinguish bundled content \
         from user-supplied MilkDrop presets; expected a line containing {LINE:?}"
    );
}

#[test]
fn credit_milkdrop_extracts_the_fixture_header_author() {
    let header = include_str!("fixtures/notices/milkdrop_ncsa.mlk");
    assert_eq!(
        credit_milkdrop(header).as_deref(),
        Some("Ada Lovelace"),
        "credit_milkdrop must read the author from the MilkDrop comment header"
    );
}

#[test]
fn specific_attribution_corrections_are_pinned() {
    let citations: &[(&str, &[&str])] = &[
        // st_Ms2SD1: the file names TDM and states NC-SA 3.0 explicitly.
        ("st_Ms2SD1.fs.fs", &["Alexander Alekseev", "TDM"]),
        // spherical-shader-tut: in-file "CC 3.0 Share Alike" with a Shadertoy
        // source cannot be less restrictive than the Shadertoy default.
        ("spherical-shader-tut.fs", &["variant not specified"]),
        // Perlin Transition: 4dS3Wd noise by Morgan McGuire.
        ("Perlin Transition.fs", &["Morgan McGuire", "4dS3Wd"]),
        // Helix Eye: iq raymarch/fog ports and the neilmendoza.com matrix.
        ("Helix Eye.fs", &["Inigo Quilez", "neilmendoza.com"]),
        // Traced Tunnel: camera/tick code retained from NuSan.
        ("Traced Tunnel.fs", &["NuSan"]),
    ];
    for (file_name, tokens) in citations {
        assert!(NOTICES.contains(file_name), "notices must name {file_name}");
        for token in *tokens {
            assert!(
                NOTICES.contains(token),
                "notices for {file_name} must contain {token:?}"
            );
        }
    }

    assert!(
        NOTICES.contains("crates/modv-renderers/src/point10/scenes/around_plus.rs"),
        "Around Plus must stay credited after the JS tree is removed"
    );
    assert!(
        NOTICES.contains("Around And Around"),
        "Around Plus must name its Plane9 preset"
    );

    assert!(
        README.contains("THIRD_PARTY_NOTICES.md"),
        "the README must link the notices"
    );
    assert!(
        !README.contains("img.shields.io/github/license"),
        "the README must not imply the bundled assets are MIT"
    );

    // No MilkDrop presets are bundled, so pin the attribution of the NC-SA
    // guard fixture instead (see milkdrop_ncsa_fixture_is_detected_and_named_in_the_notices).
    assert!(
        NOTICES.contains("Ada Lovelace"),
        "the MilkDrop notice-guard fixture's credited author must be pinned"
    );
}

#[test]
fn point10_notices_classify_every_registered_scene_by_provenance() {
    const SECTION: &str = "Point10 scenes";

    let section = notices_section(SECTION);
    let mut names: Vec<&str> = all_scenes()
        .iter()
        .map(|scene| {
            scene
                .name()
                .strip_prefix("Plane9 ")
                .or_else(|| scene.name().strip_prefix("Point10 "))
                .unwrap_or(scene.name())
        })
        .collect();
    names.sort_unstable();

    // Every registered scene needs a row in the Point10 notices, otherwise the
    // UI's licence label and the notices can disagree about its provenance.
    for name in names {
        assert!(
            section.contains(name),
            "registered Point10 scene {name:?} has no row in the notices"
        );
    }

    // Glass Lens was dropped: its upstream CodePen states no licence, and its
    // only bundled asset (pcb.jpg, unverified licence) went with it. A note
    // explaining the removal is fine; a provenance row is not.
    assert!(
        !section
            .lines()
            .any(|line| line.starts_with("| Glass Lens |")),
        "removed Glass Lens must not keep a provenance row in the notices"
    );
    assert!(
        !NOTICES.contains("pcb.jpg"),
        "the removed pcb.jpg asset must not stay in the notices"
    );

    // The old blanket claim made every scene look like a Plane9/NC-SA port.
    assert!(
        !section.contains("The Point10 scene ports are adapted from"),
        "the notices must not blanket all Point10 scenes as Plane9 ports"
    );

    for fact in [
        "Joakim Dahl",
        "Plane9",
        "three.js",
        "MIT",
        "webgpu_tsl_compute_attractors_particles",
        "Filip Zrnzevic",
        "codepen.io/filipz/pen/WbvmvBM",
        "E. T. Carter",
        "GLSL Sample027",
        "jeantimex/threejs-water",
        "first-party",
    ] {
        assert!(
            section.contains(fact),
            "the Point10 notices must record provenance fact {fact:?}"
        );
    }
}

#[test]
fn around_plus_derivative_attribution_is_pinned() {
    // AroundPlus is the repo author's Rust/wgpu port of the Plane9 "Around And
    // Around" preset by Joakim Dahl. The original CC BY-NC-SA licence and
    // attribution still bind the derivative, so both must be stated in the
    // source header and in the notices table row.
    const SOURCE: &str = include_str!("../../modv-renderers/src/point10/scenes/around_plus.rs");

    assert!(
        SOURCE.contains("CC BY-NC-SA"),
        "AroundPlus source must carry the CC BY-NC-SA licence"
    );
    assert!(
        SOURCE.contains("Joakim Dahl"),
        "AroundPlus source must credit Joakim Dahl as the original preset author"
    );

    let row = NOTICES
        .lines()
        .find(|line| line.starts_with("| Around Plus |"))
        .expect("THIRD_PARTY_NOTICES.md must have an Around Plus row");
    assert!(
        row.contains("Joakim Dahl"),
        "the Around Plus notice must credit Joakim Dahl, not claim the headers state no license"
    );
    assert!(
        row.contains("CC BY-NC-SA"),
        "the Around Plus notice must state CC BY-NC-SA"
    );
}

/// Every shader imported from Ethereios/ISF-shaders must carry the honest
/// attribution: the upstream repository's LICENSE says CC0, but its README
/// says non-commercial and the shaders are Shadertoy derivatives, so the
/// conservative CC BY-NC-SA reading applies. Each imported file names its
/// upstream source and the adapter, and each is listed in the notices.
#[test]
fn ethereios_isf_shaders_are_imported_with_attribution() {
    const UPSTREAM: &str = "github.com/Ethereios/ISF-shaders";

    let shaders = [
        "Fork of 4th eye",
        "Neon psy",
        "Rotating circles",
        "Tryptamine visions (sleeplessmonk original)",
        "bloom",
        "bulb volumetrics",
        "color fractal nayk",
        "day 15 jeyko",
        "deepseek r1 audiovisual",
        "diatribes -water cavern mod",
        "drostled",
        "evvvvil dna tracer",
        "evvvvil frankenslug",
        "fol mod",
        "fork phantom star (nikolaerceg)",
        "gamma xor",
        "iq - line fractal tunnel",
        "julia",
        "kabbalistic fractal circles",
        "kali electromagnetic politics",
        "kali inversion machine",
        "kali liberation of the true self",
        "kali psychoid mod",
        "kali the artist parody",
        "mrange - colorful failure",
        "nayk spherical remake",
        "neon psychedelic lydianlights mod",
        "nested octahedron modded",
        "phreax blue lotus",
        "phreax butterfly effect",
        "phreax hexflower",
        "phreax portal",
        "phreax twisted carbon",
        "polyhedral",
        "posthelios sky is sinus",
        "rhodium fractalscape virgil",
        "spiral riders kali",
        "tail of the dragon - diatribes",
        "torus thingy 2",
        "waves",
        "xor ascend",
    ];

    let root = repo_root();
    for stem in shaders {
        let path = format!("resources/isf/{stem}.fs");
        let source = fs::read_to_string(root.join(&path)).unwrap_or_else(|error| {
            panic!("the imported Ethereios shader {path} must exist: {error}")
        });
        for required in [
            "\"CREDIT\"",
            UPSTREAM,
            "CC BY-NC-SA 3.0",
            "THIRD_PARTY_NOTICES.md",
        ] {
            assert!(
                source.contains(required),
                "{path} must carry the attribution token {required:?}"
            );
        }
        assert!(
            NOTICES.contains(&path),
            "{path} is imported but missing from THIRD_PARTY_NOTICES.md"
        );
    }
}

#[test]
fn notices_no_longer_reference_the_deleted_electron_tree() {
    assert!(
        !NOTICES.contains("src/renderer/"),
        "THIRD_PARTY_NOTICES.md still names a deleted src/renderer path"
    );
}

/// The body of a `## <heading>` section of a markdown document, up to the next
/// `## ` heading (or end of file).
fn doc_section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let marker = format!("## {heading}");
    let start = doc
        .find(marker.as_str())
        .unwrap_or_else(|| panic!("doc is missing the section {marker:?}"));
    let after = start + marker.len();
    let end = doc[after..]
        .find("\n## ")
        .map(|offset| after + offset)
        .unwrap_or(doc.len());
    &doc[after..end]
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

#[test]
fn milkdrop_presets_guide_accepts_both_extensions_case_insensitively() {
    // The runtime discovers and watches both the legacy `.mlk` and the
    // conventional `.milk` extension case-insensitively, so the user guide must
    // say so — including in the section users read when a preset is not listed.
    const DOC: &str = "docs/milkdrop-presets.md";
    let path = repo_root().join(DOC);
    let content = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{DOC} must exist with the MilkDrop preset guidance: {error}")
    });

    let where_to_put = doc_section(&content, "Where to put MilkDrop files");
    assert!(
        contains_ci(where_to_put, ".mlk"),
        "the \"Where to put MilkDrop files\" section must still mention .mlk"
    );
    assert!(
        contains_ci(where_to_put, ".milk"),
        "the \"Where to put MilkDrop files\" section must mention the conventional \
         .milk extension: real community .milk presets can be dropped in \
         without renaming"
    );
    assert!(
        contains_ci(where_to_put, "case-insensitiv"),
        "the \"Where to put MilkDrop files\" section must state that both .mlk and \
         .milk are matched case-insensitively"
    );
    assert!(
        contains_ci(where_to_put, "renam"),
        "the \"Where to put MilkDrop files\" section must say real community .milk \
         presets can be dropped in without renaming"
    );

    let troubleshooting = doc_section(&content, "Troubleshooting");
    assert!(
        contains_ci(troubleshooting, ".mlk"),
        "the Troubleshooting section must still mention .mlk"
    );
    assert!(
        contains_ci(troubleshooting, ".milk"),
        "the Troubleshooting section must mention .milk: a user whose .milk \
         preset is not listed must find the answer there"
    );
    assert!(
        contains_ci(troubleshooting, "case-insensitiv"),
        "the Troubleshooting section must state that both .mlk and .milk are \
         accepted case-insensitively"
    );
}

/// The twelve genuine Plane9 preset ports have canonical `Plane9 ` names,
/// while their historical `Point10 ` names remain lookup aliases. This one
/// cross-crate contract covers the registry, author metadata, document-load
/// migration, historical parity fixture and licence provenance without a GPU.
#[test]
fn plane9_rename_preserves_aliases_loading_parity_and_licences() {
    const PLANE9: &[(&str, &str)] = &[
        ("Abstract Clouds", "3.0"),
        ("Blob Fade", "3.0"),
        ("Boxy", "4.0"),
        ("Circle Crossover", "3.0"),
        ("Cloud Flight", "3.0"),
        ("Color Beams", "4.0"),
        ("Color Blob", "3.0"),
        ("Color Cube", "3.0"),
        ("Colors", "4.0"),
        ("Earth", "4.0"),
        ("Gradient", "3.0"),
        ("Hollow Cube", "4.0"),
        ("It Is Coming For You", "3.0"),
        ("Light Tunnel", "3.0"),
        ("Magic Smoke Box", "3.0"),
        ("Mirror Cube", "4.0"),
        ("Mirror Tunnel", "4.0"),
        ("Noise Warp", "4.0"),
        ("Rorschach", "4.0"),
        ("Sound Cube", "4.0"),
        ("Spiky", "4.0"),
        ("Square Tunnel", "4.0"),
        ("Stargate Event Horizon", "3.0"),
        ("Sun", "3.0"),
        ("The Cave", "3.0"),
        ("Though The Noise", "3.0"),
        ("Truchet Tentacles", "3.0"),
        ("Tv Noise", "3.0"),
        ("Wormhole", "4.0"),
        ("Xor", "3.0"),
    ];
    // Only these twelve were *renamed* from a historical `Point10 ` name;
    // the ten imported presets (and this third batch's seven) never had a
    // Point10 alias.
    const RENAMED: &[(&str, &str)] = &[
        ("Boxy", "4.0"),
        ("Color Cube", "3.0"),
        ("Earth", "4.0"),
        ("Hollow Cube", "4.0"),
        ("It Is Coming For You", "3.0"),
        ("Light Tunnel", "3.0"),
        ("Mirror Cube", "4.0"),
        ("Sound Cube", "4.0"),
        ("Square Tunnel", "4.0"),
        ("Sun", "3.0"),
        ("The Cave", "3.0"),
        ("Wormhole", "4.0"),
    ];
    const POINT10: &[&str] = &[
        "Chromatic Line",
        "Chromatic Wave",
        "Particle Field",
        "Spiral Particles",
        "Waveform",
        "Water Ripple",
        "Mycelium",
        "Around Plus",
        "Ultra HDR",
        "Attractor Particles",
        "DVD Room",
        "Rolling Fire",
        "Ball Of Fire",
        "Smoke Room",
        "Latent Backrooms",
        "Hollow Cuboid",
        "Hollow Cube Layers",
        "Hollow Cube Blur",
    ];

    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($arg:tt)*) => {
            if !$condition {
                failures.push(format!($($arg)*));
            }
        };
    }

    let scenes = all_scenes();
    let scene_names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
    let expected_plane9: BTreeSet<String> = PLANE9
        .iter()
        .map(|(suffix, _)| (*suffix).to_string())
        .collect();
    let expected_point10: BTreeSet<String> =
        POINT10.iter().map(|suffix| (*suffix).to_string()).collect();
    let actual_plane9: BTreeSet<String> = scene_names
        .iter()
        .filter_map(|name| name.strip_prefix("Plane9 ").map(str::to_string))
        .collect();
    let actual_point10: BTreeSet<String> = scene_names
        .iter()
        .filter_map(|name| name.strip_prefix("Point10 ").map(str::to_string))
        .collect();

    // A — the twenty-nine Plane9 ports use the prefix; the other eighteen stay.
    check!(
        scene_names.len() == 48,
        "A: expected 48 scenes, got {}",
        scene_names.len()
    );
    check!(
        actual_plane9 == expected_plane9,
        "A: Plane9 registry suffixes differ: missing {:?}, unexpected {:?}",
        expected_plane9
            .difference(&actual_plane9)
            .collect::<Vec<_>>(),
        actual_plane9
            .difference(&expected_plane9)
            .collect::<Vec<_>>()
    );
    check!(
        actual_point10 == expected_point10,
        "A: Point10 registry suffixes differ: missing {:?}, unexpected {:?}",
        expected_point10
            .difference(&actual_point10)
            .collect::<Vec<_>>(),
        actual_point10
            .difference(&expected_point10)
            .collect::<Vec<_>>()
    );

    // B — author metadata follows the canonical per-name provenance.
    let mut author_mismatches = Vec::new();
    for (suffix, _) in PLANE9 {
        let name = format!("Plane9 {suffix}");
        match scenes.iter().find(|scene| scene.name() == name) {
            Some(scene) if scene.meta().author == "Plane9 (Joakim Dahl)" => {}
            Some(scene) => author_mismatches.push(format!(
                "{name}: author {:?}, expected Plane9 (Joakim Dahl)",
                scene.meta().author
            )),
            None => author_mismatches.push(format!("{name}: missing")),
        }
    }
    for suffix in POINT10 {
        let name = format!("Point10 {suffix}");
        match scenes.iter().find(|scene| scene.name() == name) {
            Some(scene) if scene.meta().author == "Point10" => {}
            Some(scene) => author_mismatches.push(format!(
                "{name}: author {:?}, expected Point10",
                scene.meta().author
            )),
            None => author_mismatches.push(format!("{name}: missing")),
        }
    }
    check!(
        author_mismatches.is_empty(),
        "B: author metadata mismatches: {}",
        author_mismatches.join("; ")
    );

    // C — every historical name resolves to the same canonical definition and
    // constructor result as its new Plane9 name.
    let registry = ModuleRegistry::builtin();
    let mut alias_mismatches = Vec::new();
    for (suffix, _) in RENAMED {
        let old_name = format!("Point10 {suffix}");
        let new_name = format!("Plane9 {suffix}");
        let canonical = registry.point10_definition(&new_name);
        let legacy = registry.point10_definition(&old_name);
        let canonical_scene = registry
            .create_point10_instance(&new_name)
            .map(|scene| scene.name());
        let legacy_scene = registry
            .create_point10_instance(&old_name)
            .map(|scene| scene.name());
        let mut problems = Vec::new();
        if canonical.is_none() {
            problems.push("new definition missing");
        }
        if canonical != legacy {
            problems.push("old/new definitions differ");
        }
        if registry.kind(&new_name) != Some(ModuleKind::Point10)
            || registry.kind(&old_name) != Some(ModuleKind::Point10)
        {
            problems.push("old/new renderer kind is not Point10");
        }
        if canonical_scene != Some(new_name.as_str()) || legacy_scene != canonical_scene {
            problems.push("old/new constructors do not produce the canonical scene");
        }
        if !problems.is_empty() {
            alias_mismatches.push(format!("{suffix}: {}", problems.join(", ")));
        }
    }
    check!(
        alias_mismatches.is_empty(),
        "C: legacy alias mismatches: {}",
        alias_mismatches.join("; ")
    );

    // D — both preset restore and Scene import rewrite the persisted reference,
    // not merely the live renderer lookup, so the next save uses the new name.
    let legacy_document = json!({
        "id": "scene-legacy-plane9",
        "name": "Legacy Plane9",
        "groups": [{
            "id": "group-legacy",
            "name": "Legacy",
            "hidden": false,
            "modules": ["light-tunnel"],
            "enabled": 1,
            "alpha": 1,
            "inherit": true,
            "inheritFrom": -1,
            "pipeline": false,
            "clearing": false,
            "compositeOperation": "normal"
        }],
        "modules": {
            "light-tunnel": {
                "$id": "light-tunnel",
                "$moduleName": "Point10 Light Tunnel",
                "meta": {"name": "Point10 Light Tunnel"},
                "props": {},
                "data": {},
                "$props": {}
            }
        },
        "inputs": {},
        "expressions": {},
        "tweens": [],
        "midi": {}
    });

    let mut preset_store = Store::new();
    match preset_store.restore(&legacy_document) {
        Ok(()) => {
            if let Some(module) = preset_store.state().modules.get("light-tunnel") {
                check!(
                    module.module_name == "Plane9 Light Tunnel",
                    "D: preset restore kept $moduleName {:?}",
                    module.module_name
                );
                check!(
                    module.status.is_empty(),
                    "D: preset restore produced module status {:?}",
                    module.status
                );
                check!(
                    preset_store.registry_arc().kind(&module.module_name)
                        == Some(ModuleKind::Point10),
                    "D: restored preset module {:?} does not resolve as Point10",
                    module.module_name
                );
            } else {
                check!(false, "D: preset restore dropped light-tunnel");
            }
            let saved = preset_store.snapshot();
            check!(
                saved["modules"]["light-tunnel"]["$moduleName"] == "Plane9 Light Tunnel",
                "D: preset re-save wrote $moduleName {:?}",
                saved["modules"]["light-tunnel"]["$moduleName"]
            );
        }
        Err(error) => check!(false, "D: preset restore failed: {error:?}"),
    }

    let mut scene_store = Store::new();
    let scene_params = json!({"scene": legacy_document, "persist": false});
    match scene_store.load_scene_from_json(scene_params.as_object().expect("Scene params")) {
        Ok(imported) => {
            check!(
                imported["modules"]["light-tunnel"]["$moduleName"] == "Plane9 Light Tunnel",
                "D: Scene import returned $moduleName {:?}",
                imported["modules"]["light-tunnel"]["$moduleName"]
            );
            let stored = scene_store
                .scenes_state()
                .scenes
                .iter()
                .find(|scene| scene["id"] == "scene-legacy-plane9");
            check!(stored.is_some(), "D: Scene import did not store the Scene");
            if let Some(stored) = stored {
                check!(
                    stored["modules"]["light-tunnel"]["$moduleName"] == "Plane9 Light Tunnel",
                    "D: stored Scene kept $moduleName {:?}",
                    stored["modules"]["light-tunnel"]["$moduleName"]
                );
            }
            let live_modules: Vec<_> = scene_store.state().modules.values().collect();
            check!(
                live_modules.len() == 1,
                "D: Scene apply created {} modules, expected 1",
                live_modules.len()
            );
            if let Some(module) = live_modules.first() {
                check!(
                    module.module_name == "Plane9 Light Tunnel",
                    "D: Scene apply kept live $moduleName {:?}",
                    module.module_name
                );
                check!(
                    module.status.is_empty(),
                    "D: Scene apply produced module status {:?}",
                    module.status
                );
                check!(
                    scene_store.registry_arc().kind(&module.module_name)
                        == Some(ModuleKind::Point10),
                    "D: Scene-applied module {:?} does not resolve as Point10",
                    module.module_name
                );
            }
            let sync = scene_store.sync_state();
            check!(
                sync["scenes"][0]["modules"]["light-tunnel"]["$moduleName"]
                    == "Plane9 Light Tunnel",
                "D: Scene sync/re-save wrote $moduleName {:?}",
                sync["scenes"][0]["modules"]["light-tunnel"]["$moduleName"]
            );
        }
        Err(error) => check!(false, "D: Scene import/apply failed: {error:?}"),
    }

    // E — the fixture remains the historical legacy inventory. Compare it to
    // canonical registry names only after applying the legacy alias mapping.
    let manifest: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/module-parity.json"))
            .expect("the historical parity manifest is valid JSON");
    let manifest_names: BTreeSet<String> = manifest["point10"]
        .as_array()
        .expect("point10 manifest array")
        .iter()
        .map(|name| name.as_str().expect("module name").to_string())
        .collect();
    let mut manifest_mismatches = Vec::new();
    if manifest_names.len() != 48 {
        manifest_mismatches.push(format!("expected 48 names, got {}", manifest_names.len()));
    }
    for (suffix, _) in RENAMED {
        let old_name = format!("Point10 {suffix}");
        let new_name = format!("Plane9 {suffix}");
        if !manifest_names.contains(&old_name) {
            manifest_mismatches.push(format!("historical {old_name} missing"));
        }
        if manifest_names.contains(&new_name) {
            manifest_mismatches.push(format!("historical fixture was rewritten to {new_name}"));
        }
    }
    check!(
        manifest_mismatches.is_empty(),
        "E: historical manifest mismatches: {}",
        manifest_mismatches.join("; ")
    );

    let canonical_for_legacy = |name: &str| {
        RENAMED
            .iter()
            .find_map(|(suffix, _)| {
                (name == format!("Point10 {suffix}")).then(|| format!("Plane9 {suffix}"))
            })
            .unwrap_or_else(|| name.to_string())
    };
    let expected_canonical: BTreeSet<String> = manifest_names
        .iter()
        .map(|name| canonical_for_legacy(name))
        .collect();
    let actual_canonical: BTreeSet<String> = registry
        .point10_definitions()
        .iter()
        .map(|definition| definition.meta.name.to_string())
        .collect();
    check!(
        actual_canonical == expected_canonical,
        "E: alias-normalised registry differs from historical manifest: missing {:?}, unexpected {:?}",
        expected_canonical
            .difference(&actual_canonical)
            .collect::<Vec<_>>(),
        actual_canonical
            .difference(&expected_canonical)
            .collect::<Vec<_>>()
    );
    let unresolved_legacy: Vec<_> = manifest_names
        .iter()
        .filter(|name| registry.point10_definition(name).is_none())
        .collect();
    check!(
        unresolved_legacy.is_empty(),
        "E: historical manifest aliases do not resolve: {unresolved_legacy:?}"
    );

    // F — the UI licence map and notices provenance rows keep the exact terms
    // each port had before its display name changed.
    let notices = notices_section("Point10 scenes");
    let mut licence_mismatches = Vec::new();
    let mut notice_mismatches = Vec::new();
    for (suffix, version) in PLANE9 {
        let name = format!("Plane9 {suffix}");
        let expected =
            format!("CC BY-NC-SA {version} (Plane9 / Joakim Dahl; see THIRD_PARTY_NOTICES.md)");
        let licence = module_license(&ModuleMetaView {
            name: name.clone(),
            module_type: "2d".to_string(),
            ..ModuleMetaView::default()
        });
        if licence != expected {
            licence_mismatches.push(format!("{name}: got {licence:?}, expected {expected:?}"));
        }
        let row = notices
            .lines()
            .find(|line| line.starts_with(&format!("| {suffix} |")));
        match row {
            Some(row) if row.contains(&format!("CC BY-NC-SA {version}")) => {}
            Some(row) => notice_mismatches.push(format!("{name}: wrong row {row:?}")),
            None => notice_mismatches.push(format!("{name}: no provenance row")),
        }
    }
    for scene in &scenes {
        let suffix = scene
            .name()
            .strip_prefix("Plane9 ")
            .or_else(|| scene.name().strip_prefix("Point10 "))
            .unwrap_or(scene.name());
        if !notices
            .lines()
            .any(|line| line.starts_with(&format!("| {suffix} |")))
        {
            notice_mismatches.push(format!(
                "{}: no suffix-matched provenance row",
                scene.name()
            ));
        }
    }
    check!(
        licence_mismatches.is_empty(),
        "F: licence mismatches: {}",
        licence_mismatches.join("; ")
    );
    check!(
        notice_mismatches.is_empty(),
        "F: notices provenance mismatches: {}",
        notice_mismatches.join("; ")
    );

    assert!(
        failures.is_empty(),
        "Plane9 rename contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Render one Plane9 batch scene through the same two-frame presentation path
/// as `point10::scene_tests`, returning tightly packed 64x64 RGBA8 pixels.
fn render_plane9_batch_smoke(scene: &mut dyn Point10Scene, engine: &Engine) -> Vec<u8> {
    const SIZE: u32 = 64;

    let mut composer = Composer::new(
        &engine.device,
        &engine.queue,
        SIZE,
        SIZE,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let mut ctx = SceneContext::new(&engine.device, &engine.queue, HDR_FORMAT);
    ctx.resize(SIZE, SIZE);
    scene.init(&mut ctx);
    scene.resize(SIZE, SIZE);

    let mut payload = BandPayload::silent(44_100.0);
    payload.byte_frequency_data = vec![200; 1024];
    payload.wave = [0.5; AUDIO_SAMPLES];
    payload.beat = 0.8;
    payload.lanes = [0.6; 12];
    payload.cells = [0.6; 16];
    payload.freq_mag = [0.4; AUDIO_SAMPLES];
    let mut bridge = AudioBridge::new();

    let texture = engine.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("plane9-batch-smoke-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = engine
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("plane9-batch-smoke"),
        });
    for _ in 0..2 {
        bridge.update(1.0 / 60.0, Some(&payload), &Default::default());
        scene.update(1.0 / 60.0, &bridge);
        scene.render(&mut encoder, composer.scene_view());
    }
    composer.render(&mut encoder, &view);
    engine.queue.submit(Some(encoder.finish()));

    // 64 * 4 is already 256-byte aligned, so the tightly packed readback needs
    // no row-padding removal.
    let bytes_per_row = SIZE * 4;
    let buffer = engine.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("plane9-batch-smoke-readback"),
        size: bytes_per_row as u64 * SIZE as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = engine
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("plane9-batch-smoke-readback"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    engine.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    engine
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll the Plane9 smoke readback");
    receiver
        .recv()
        .expect("the Plane9 readback callback runs")
        .expect("map the Plane9 readback");
    let mapped = slice
        .get_mapped_range()
        .expect("the Plane9 readback buffer is mapped");
    let pixels = mapped.to_vec();
    drop(mapped);
    buffer.unmap();
    scene.dispose();
    pixels
}

/// The ten simple Plane9 XML presets imported as one batch. This deliberately
/// resolves scenes by registry name rather than naming their not-yet-existing
/// Rust constructors, so the RED test compiles before the port exists.
#[test]
#[allow(clippy::op_ref)]
fn plane9_batch_ten_registry_provenance_catalogue_and_gpu_contract() {
    const PRESETS: &[(&str, &str)] = &[
        ("Colors", "4.0"),
        ("Gradient", "3.0"),
        ("Noise Warp", "4.0"),
        ("Rorschach", "4.0"),
        ("Xor", "3.0"),
        ("Truchet Tentacles", "3.0"),
        ("Blob Fade", "3.0"),
        ("Though The Noise", "3.0"),
        ("Tv Noise", "3.0"),
        ("Circle Crossover", "3.0"),
    ];

    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($arg:tt)*) => {
            if !$condition {
                failures.push(format!($($arg)*));
            }
        };
    }

    let mut scenes = all_scenes();
    let names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
    let unique_names: BTreeSet<&str> = names.iter().copied().collect();
    check!(
        scenes.len() == 48,
        "roster: expected 48 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "roster: duplicate names in {names:?}"
    );
    check!(
        modv_renderers::point10::PLANE9_SCENE_NAMES.len() == 30,
        "Plane9 alias roster: expected 30 names, got {}",
        modv_renderers::point10::PLANE9_SCENE_NAMES.len()
    );

    for (suffix, version) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let matches: Vec<_> = scenes.iter().filter(|scene| scene.name() == name).collect();
        check!(
            matches.len() == 1,
            "{name}: expected exactly one all_scenes registration, got {}",
            matches.len()
        );
        if let Some(scene) = matches.first() {
            let meta = scene.meta();
            check!(
                meta.name == scene.name(),
                "{name}: meta.name does not match name()"
            );
            check!(
                meta.author == "Plane9 (Joakim Dahl)",
                "{name}: author {:?}, expected Plane9 (Joakim Dahl)",
                meta.author
            );
            check!(
                meta.version == "1.0.0",
                "{name}: version {:?}",
                meta.version
            );
            check!(
                meta.module_type == "2d",
                "{name}: type {:?}",
                meta.module_type
            );
            check!(
                meta.audio_features == &["bass", "mid", "treble"],
                "{name}: audio features {:?}",
                meta.audio_features
            );
        }
        check!(
            modv_renderers::point10::PLANE9_SCENE_NAMES.contains(&name.as_str()),
            "{name}: missing from the Plane9 alias/Gallery roster"
        );
        check!(
            modv_renderers::point10::gallery_kind(&name, "point10") == "plane9",
            "{name}: Gallery kind is not plane9"
        );

        let expected_license =
            format!("CC BY-NC-SA {version} (Plane9 / Joakim Dahl; see THIRD_PARTY_NOTICES.md)");
        let license = module_license(&ModuleMetaView {
            name: name.clone(),
            module_type: "2d".to_string(),
            kind: "plane9".to_string(),
            ..ModuleMetaView::default()
        });
        check!(
            license == expected_license,
            "{name}: UI license {license:?}, expected {expected_license:?}"
        );
        let notices = notices_section("Point10 scenes");
        let row = notices
            .lines()
            .find(|line| line.starts_with(&format!("| {suffix} |")));
        match row {
            Some(row) if row.contains(&format!("CC BY-NC-SA {version}")) => {}
            Some(row) => check!(false, "{name}: wrong notices row {row:?}"),
            None => check!(false, "{name}: missing notices provenance row"),
        }
    }

    let registry = ModuleRegistry::builtin();
    check!(
        registry.point10_definitions().len() == 48,
        "ModuleRegistry: expected 48 Point10 definitions, got {}",
        registry.point10_definitions().len()
    );
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        check!(
            registry.kind(&name) == Some(ModuleKind::Point10),
            "{name}: ModuleRegistry kind is not Point10"
        );
        match registry.point10_definition(&name) {
            Some(definition) => {
                check!(
                    definition.meta.author == "Plane9 (Joakim Dahl)"
                        && definition.meta.version == "1.0.0"
                        && definition.meta.module_type == "2d"
                        && definition.meta.audio_features == &["bass", "mid", "treble"],
                    "{name}: ModuleRegistry metadata does not match the Plane9 contract"
                );
            }
            None => check!(false, "{name}: missing from ModuleRegistry"),
        }
    }

    let manifest: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/module-parity.json"))
            .expect("the parity manifest is valid JSON");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(48),
        "manifest: point10 count is {:?}, expected 48",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(502),
        "manifest: total is {:?}, expected 502",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 48,
        "manifest: expected 48 Point10 names, got {}",
        manifest_names.len()
    );
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        check!(
            manifest_names
                .iter()
                .filter(|candidate| **candidate == name)
                .count()
                == 1,
            "{name}: parity manifest must contain the canonical name exactly once"
        );
    }

    let corpus = repo_root().join("resources/isf");
    let generated = build_document(&corpus);
    let (metadata, previews) = split_documents(&generated);
    let committed: Value = serde_json::from_str(include_str!("../../../docs/modules/modules.json"))
        .expect("the committed module catalogue is valid JSON");
    check!(
        metadata == committed,
        "docs/modules/modules.json does not match the metadata projection of build_document()"
    );
    let committed_previews: Value =
        serde_json::from_str(include_str!("../../../docs/modules/previews.json"))
            .expect("the committed preview catalogue is valid JSON");
    check!(
        previews == committed_previews,
        "docs/modules/previews.json does not match the preview projection of build_document()"
    );
    check!(
        generated["counts"]["point10"].as_u64() == Some(48),
        "catalogue: point10 count is {:?}, expected 48",
        generated["counts"]["point10"]
    );
    check!(
        generated["counts"]["total"].as_u64() == Some(502),
        "catalogue: total is {:?}, expected 502",
        generated["counts"]["total"]
    );
    let modules = generated["modules"]
        .as_array()
        .expect("catalogue modules array");
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let matches: Vec<_> = modules
            .iter()
            .filter(|module| module["name"] == name)
            .collect();
        check!(
            matches.len() == 1,
            "{name}: catalogue must contain the module exactly once, got {}",
            matches.len()
        );
        if let Some(module) = matches.first() {
            check!(
                module["kind"] == "point10"
                    && module["author"] == "Plane9 (Joakim Dahl)"
                    && module["version"] == "1.0.0"
                    && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"]),
                "{name}: catalogue metadata does not match the Plane9 contract"
            );
        }
    }

    for face in ["posx", "negx", "posy", "negy", "posz", "negz"] {
        let path = repo_root().join(format!("resources/point10/textures/stormy/{face}.jpg"));
        check!(
            path.is_file(),
            "Though The Noise: missing bundled cubemap face {path:?}"
        );
    }

    assert!(
        failures.is_empty(),
        "Plane9 batch contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );

    let Ok(engine) = Engine::new(&EngineConfig {
        force_fallback_adapter: true,
    }) else {
        eprintln!("plane9 batch GPU smoke skipped: no fallback adapter");
        return;
    };
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let index = scenes
            .iter()
            .position(|scene| scene.name() == name)
            .expect("the registry contract above guarantees the scene exists");
        let pixels = render_plane9_batch_smoke(scenes[index].as_mut(), &engine);
        let non_black = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0);
        check!(
            non_black,
            "{name}: two-frame 64x64 fallback-adapter render is uniformly black"
        );
    }
    assert!(
        failures.is_empty(),
        "Plane9 batch GPU smoke failed:\n{}",
        failures.join("\n")
    );
}

/// The second XML batch: seven RTT/feedback/raymarch presets. Resolve scenes
/// through the public registry instead of naming their future Rust constructors,
/// so this RED contract compiles before any of the seven ports exists.
#[test]
#[allow(clippy::op_ref)]
fn plane9_batch_two_seven_registry_provenance_catalogue_and_gpu_contract() {
    const PRESETS: &[(&str, &str)] = &[
        ("Abstract Clouds", "3.0"),
        ("Magic Smoke Box", "3.0"),
        ("Color Blob", "3.0"),
        ("Stargate Event Horizon", "3.0"),
        ("Color Beams", "4.0"),
        ("Mirror Tunnel", "4.0"),
        ("Cloud Flight", "3.0"),
    ];

    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($arg:tt)*) => {
            if !$condition {
                failures.push(format!($($arg)*));
            }
        };
    }

    let mut scenes = all_scenes();
    let names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
    let unique_names: BTreeSet<&str> = names.iter().copied().collect();
    check!(
        scenes.len() == 48,
        "roster: expected 48 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "roster: duplicate names in {names:?}"
    );

    let aliases = modv_renderers::point10::PLANE9_SCENE_NAMES;
    let unique_aliases: BTreeSet<&str> = aliases.iter().copied().collect();
    check!(
        aliases.len() == 30,
        "Plane9 alias roster: expected 30 names, got {}",
        aliases.len()
    );
    check!(
        unique_aliases.len() == aliases.len(),
        "Plane9 alias roster: duplicate names in {aliases:?}"
    );

    for (suffix, version) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let matches: Vec<_> = scenes.iter().filter(|scene| scene.name() == name).collect();
        check!(
            matches.len() == 1,
            "{name}: expected exactly one all_scenes registration, got {}",
            matches.len()
        );
        if let Some(scene) = matches.first() {
            let meta = scene.meta();
            check!(
                meta.name == scene.name(),
                "{name}: meta.name does not match name()"
            );
            check!(
                meta.author == "Plane9 (Joakim Dahl)",
                "{name}: author {:?}, expected Plane9 (Joakim Dahl)",
                meta.author
            );
            check!(
                meta.version == "1.0.0",
                "{name}: version {:?}",
                meta.version
            );
            check!(
                meta.module_type == "2d",
                "{name}: type {:?}",
                meta.module_type
            );
            check!(
                meta.audio_features == &["bass", "mid", "treble"],
                "{name}: audio features {:?}",
                meta.audio_features
            );
        }
        check!(
            aliases
                .iter()
                .filter(|candidate| **candidate == name)
                .count()
                == 1,
            "{name}: Plane9 alias/Gallery roster must contain the canonical name exactly once"
        );
        let legacy = format!("Point10 {suffix}");
        check!(
            modv_renderers::point10::canonical_scene_name(&legacy) == Some(name.as_str()),
            "{legacy}: historical alias must resolve to {name}"
        );
        check!(
            modv_renderers::point10::gallery_kind(&name, "point10") == "plane9",
            "{name}: Gallery kind is not plane9"
        );

        let expected_license =
            format!("CC BY-NC-SA {version} (Plane9 / Joakim Dahl; see THIRD_PARTY_NOTICES.md)");
        let license = module_license(&ModuleMetaView {
            name: name.clone(),
            module_type: "2d".to_string(),
            kind: "plane9".to_string(),
            ..ModuleMetaView::default()
        });
        check!(
            license == expected_license,
            "{name}: UI license {license:?}, expected {expected_license:?}"
        );
        let notices = notices_section("Point10 scenes");
        let rows: Vec<_> = notices
            .lines()
            .filter(|line| line.starts_with(&format!("| {suffix} |")))
            .collect();
        match rows.as_slice() {
            [row] if row.contains(&format!("CC BY-NC-SA {version}")) => {}
            [row] => check!(false, "{name}: wrong notices row {row:?}"),
            [] => check!(false, "{name}: missing notices provenance row"),
            _ => check!(false, "{name}: duplicate notices provenance rows {rows:?}"),
        }
    }

    let registry = ModuleRegistry::builtin();
    check!(
        registry.point10_definitions().len() == 48,
        "ModuleRegistry: expected 48 Point10 definitions, got {}",
        registry.point10_definitions().len()
    );
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        check!(
            registry.kind(&name) == Some(ModuleKind::Point10),
            "{name}: ModuleRegistry kind is not Point10"
        );
        match registry.point10_definition(&name) {
            Some(definition) => {
                check!(
                    definition.meta.name == name
                        && definition.meta.author == "Plane9 (Joakim Dahl)"
                        && definition.meta.version == "1.0.0"
                        && definition.meta.module_type == "2d"
                        && definition.meta.audio_features == &["bass", "mid", "treble"],
                    "{name}: ModuleRegistry metadata does not match the Plane9 contract"
                );
            }
            None => check!(false, "{name}: missing from ModuleRegistry"),
        }
        check!(
            registry
                .create_point10_instance(&name)
                .map(|scene| scene.name())
                == Some(name.as_str()),
            "{name}: ModuleRegistry cannot construct the canonical scene"
        );
        let legacy = format!("Point10 {suffix}");
        check!(
            registry
                .create_point10_instance(&legacy)
                .map(|scene| scene.name())
                == Some(name.as_str()),
            "{legacy}: ModuleRegistry alias cannot construct {name}"
        );
    }

    let manifest: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/module-parity.json"))
            .expect("the parity manifest is valid JSON");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(48),
        "manifest: point10 count is {:?}, expected 48",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(502),
        "manifest: total is {:?}, expected 502",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 48,
        "manifest: expected 48 Point10 names, got {}",
        manifest_names.len()
    );
    let unique_manifest_names: BTreeSet<&str> = manifest_names.iter().copied().collect();
    check!(
        unique_manifest_names.len() == manifest_names.len(),
        "manifest: duplicate Point10 names"
    );
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        check!(
            manifest_names
                .iter()
                .filter(|candidate| **candidate == name)
                .count()
                == 1,
            "{name}: parity manifest must contain the canonical name exactly once"
        );
    }

    let corpus = repo_root().join("resources/isf");
    let generated = build_document(&corpus);
    let (metadata, previews) = split_documents(&generated);
    let committed: Value = serde_json::from_str(include_str!("../../../docs/modules/modules.json"))
        .expect("the committed module catalogue is valid JSON");
    check!(
        metadata == committed,
        "docs/modules/modules.json does not match the metadata projection of build_document()"
    );
    let committed_previews: Value =
        serde_json::from_str(include_str!("../../../docs/modules/previews.json"))
            .expect("the committed preview catalogue is valid JSON");
    check!(
        previews == committed_previews,
        "docs/modules/previews.json does not match the preview projection of build_document()"
    );
    check!(
        generated["counts"]["point10"].as_u64() == Some(48),
        "catalogue: point10 count is {:?}, expected 48",
        generated["counts"]["point10"]
    );
    check!(
        generated["counts"]["total"].as_u64() == Some(502),
        "catalogue: total is {:?}, expected 502",
        generated["counts"]["total"]
    );
    let modules = generated["modules"]
        .as_array()
        .expect("catalogue modules array");
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let matches: Vec<_> = modules
            .iter()
            .filter(|module| module["name"] == name)
            .collect();
        check!(
            matches.len() == 1,
            "{name}: catalogue must contain the module exactly once, got {}",
            matches.len()
        );
        if let Some(module) = matches.first() {
            check!(
                module["kind"] == "point10"
                    && module["author"] == "Plane9 (Joakim Dahl)"
                    && module["version"] == "1.0.0"
                    && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"]),
                "{name}: catalogue metadata does not match the Plane9 contract"
            );
        }
    }

    assert!(
        failures.is_empty(),
        "Plane9 seven-scene batch contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );

    let engine = Engine::new(&EngineConfig {
        force_fallback_adapter: true,
    })
    .expect("the Plane9 seven-scene GPU smoke requires the headless fallback adapter");
    for (suffix, _) in PRESETS {
        let name = format!("Plane9 {suffix}");
        let index = scenes
            .iter()
            .position(|scene| scene.name() == name)
            .expect("the registry contract above guarantees the scene exists");
        let pixels = render_plane9_batch_smoke(scenes[index].as_mut(), &engine);
        check!(
            pixels.len() == 64 * 64 * 4,
            "{name}: expected 64x64 RGBA8 readback, got {} bytes",
            pixels.len()
        );
        let non_black = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0);
        let non_transparent = pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0);
        check!(
            non_black && non_transparent,
            "{name}: two-frame 64x64 fallback-adapter render is empty or uniformly black"
        );
    }
    assert!(
        failures.is_empty(),
        "Plane9 seven-scene GPU smoke failed:\n{}",
        failures.join("\n")
    );
}

/// The audited Plane9 shader effects and ISF transitions keep their historical
/// `Point10 <Name>` references loadable while registering under canonical
/// `Plane9 <Name>` names. This cross-crate contract covers the registry,
/// metadata, aliases, licences, notices, corpus baseline and generated catalogue
/// without requiring a GPU.
#[test]
fn plane9_effect_rename_preserves_registry_aliases_metadata_licences_and_catalogue() {
    const SHADER_EFFECTS: &[(&str, &str)] = &[
        ("Gray", "4.0"),
        ("Invert", "4.0"),
        ("Posterization", "4.0"),
        ("Film Grain", "4.0"),
        ("BlackAndWhite", "4.0"),
        ("Gray Stripes", "4.0"),
        ("Invert Gray", "4.0"),
        ("Crosshatching", "4.0"),
        ("Blocks", "4.0"),
        ("3d Blocks", "3.0"),
        ("Bayer 8 Matrix Dithering BW", "3.0"),
        ("Bayer Matrix Dithering Color", "3.0"),
        ("Fuzzy Pixels", "4.0"),
        ("Fractured", "4.0"),
        ("Bathroom Window", "4.0"),
        ("Contrast", "3.0"),
        ("Color Distort", "4.0"),
        ("Crt", "4.0"),
        ("Cubic Lens Distortion", "3.0"),
        ("Hexagons", "4.0"),
        ("Led Display", "4.0"),
        ("Led Display Curved", "4.0"),
        ("Limit Hue", "4.0"),
        ("Embos Merge", "4.0"),
        ("Embos", "3.0"),
    ];
    const ISF_TRANSITIONS: &[(&str, &str)] = &[
        ("Banded Swirl", "3.0"),
        ("Blinds", "3.0"),
        ("Block", "4.0"),
        ("Circle Reveal", "3.0"),
        ("Circle Stretch", "3.0"),
        ("Clock Swipe", "4.0"),
        ("Close Horizontal", "4.0"),
        ("Crossfade", "3.0"),
        ("Crumble", "3.0"),
        ("Disolve", "3.0"),
        ("Dropfade", "3.0"),
        ("Erode", "3.0"),
        ("Line Reveal", "3.0"),
        ("Lines", "3.0"),
        ("Pixelate", "3.0"),
        ("Pour", "3.0"),
        ("Radial Blur", "3.0"),
        ("Random Circle", "3.0"),
        ("Ripple", "3.0"),
        ("Shockwave", "3.0"),
        ("Shrink", "3.0"),
        ("SlideIn", "3.0"),
        ("Smooth Swirl Grid", "3.0"),
        ("Swirl", "3.0"),
        ("Swirl Grid", "3.0"),
        ("Water", "3.0"),
        ("Wave", "4.0"),
    ];
    const POINT10_SHADER_EFFECTS: &[&str] = &[
        "Pixelation",
        "RGB Shift",
        "Digital Glitch",
        "Vignette",
        "Bloom",
    ];

    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($arg:tt)*) => {
            if !$condition {
                failures.push(format!($($arg)*));
            }
        };
    }
    let summarize = |problems: &[String]| {
        let shown: Vec<&str> = problems.iter().take(3).map(String::as_str).collect();
        if problems.len() > shown.len() {
            format!("{shown:?}; +{} more", problems.len() - shown.len())
        } else {
            format!("{shown:?}")
        }
    };

    let corpus = repo_root().join("resources/isf");
    let registry = ModuleRegistry::build(Some(&corpus));
    let renamed: Vec<(&str, &str)> = SHADER_EFFECTS
        .iter()
        .chain(ISF_TRANSITIONS.iter())
        .copied()
        .collect();
    let legacy_names: Vec<String> = renamed
        .iter()
        .map(|(suffix, _)| format!("Point10 {suffix}"))
        .collect();
    let canonical_names: Vec<String> = renamed
        .iter()
        .map(|(suffix, _)| format!("Plane9 {suffix}"))
        .collect();

    // A — exactly the audited effects move prefix, with unchanged bucket sizes
    // and no duplicate registration inside or across renderer buckets.
    let expected_shader_plane9: BTreeSet<String> = SHADER_EFFECTS
        .iter()
        .map(|(suffix, _)| (*suffix).to_string())
        .collect();
    let actual_shader_plane9: BTreeSet<String> = registry
        .shader_modules()
        .iter()
        .filter_map(|module| module.name.strip_prefix("Plane9 ").map(str::to_string))
        .collect();
    let expected_shader_point10: BTreeSet<String> = POINT10_SHADER_EFFECTS
        .iter()
        .map(|suffix| (*suffix).to_string())
        .collect();
    let actual_shader_point10: BTreeSet<String> = registry
        .shader_modules()
        .iter()
        .filter_map(|module| module.name.strip_prefix("Point10 ").map(str::to_string))
        .collect();
    let expected_isf_plane9: BTreeSet<String> = ISF_TRANSITIONS
        .iter()
        .map(|(suffix, _)| (*suffix).to_string())
        .collect();
    let actual_isf_plane9: BTreeSet<String> = registry
        .isf_definitions()
        .iter()
        .filter_map(|definition| definition.name.strip_prefix("Plane9 ").map(str::to_string))
        .collect();
    let actual_isf_point10: BTreeSet<String> = registry
        .isf_definitions()
        .iter()
        .filter_map(|definition| definition.name.strip_prefix("Point10 ").map(str::to_string))
        .collect();

    check!(
        registry.shader_modules().len() == 36,
        "A: shader bucket has {} modules, expected 36",
        registry.shader_modules().len()
    );
    check!(
        registry.isf_definitions().len() == 402,
        "A: ISF bucket has {} modules, expected 402",
        registry.isf_definitions().len()
    );
    check!(
        actual_shader_plane9 == expected_shader_plane9,
        "A: shader Plane9 roster has {} missing and {} unexpected suffixes",
        expected_shader_plane9
            .difference(&actual_shader_plane9)
            .count(),
        actual_shader_plane9
            .difference(&expected_shader_plane9)
            .count()
    );
    check!(
        actual_shader_point10 == expected_shader_point10,
        "A: shader Point10 roster has {} missing and {} unexpected suffixes",
        expected_shader_point10
            .difference(&actual_shader_point10)
            .count(),
        actual_shader_point10
            .difference(&expected_shader_point10)
            .count()
    );
    check!(
        actual_isf_plane9 == expected_isf_plane9,
        "A: ISF Plane9 roster has {} missing and {} unexpected suffixes",
        expected_isf_plane9.difference(&actual_isf_plane9).count(),
        actual_isf_plane9.difference(&expected_isf_plane9).count()
    );
    check!(
        actual_isf_point10.is_empty(),
        "A: {} ISF transitions still use the Point10 prefix",
        actual_isf_point10.len()
    );

    let mut registered_names: Vec<String> = registry
        .shader_modules()
        .iter()
        .map(|module| module.name.to_string())
        .collect();
    registered_names.extend(
        registry
            .two_d_definitions()
            .iter()
            .map(|definition| definition.meta.name.to_string()),
    );
    registered_names.extend(
        registry
            .point10_definitions()
            .iter()
            .map(|definition| definition.meta.name.to_string()),
    );
    registered_names.extend(
        registry
            .isf_definitions()
            .iter()
            .map(|definition| definition.name.clone()),
    );
    let unique_registered_names: BTreeSet<String> = registered_names.iter().cloned().collect();
    check!(
        unique_registered_names.len() == registered_names.len(),
        "A: {} duplicate cross-bucket registrations",
        registered_names.len() - unique_registered_names.len()
    );

    // B/C — canonical metadata is Plane9/Dahl, and every historical spelling
    // resolves to the very same shader or ISF definition.
    let mut metadata_mismatches = Vec::new();
    let mut alias_mismatches = Vec::new();
    for (suffix, _) in SHADER_EFFECTS {
        let name = format!("Plane9 {suffix}");
        let legacy_name = format!("Point10 {suffix}");
        let Some(canonical) = registry.shader_module(&name) else {
            metadata_mismatches.push(format!("{name}: missing"));
            alias_mismatches.push(format!("{legacy_name}: canonical module missing"));
            continue;
        };
        if canonical.meta_author != "Plane9 (Joakim Dahl)" {
            metadata_mismatches.push(format!("{name}: author {:?}", canonical.meta_author));
        }
        let fields = registry.meta_fields(&name);
        if !fields.is_some_and(|fields| {
            fields.get("name").and_then(Value::as_str) == Some(name.as_str())
                && fields.get("author").and_then(Value::as_str) == Some("Plane9 (Joakim Dahl)")
        }) {
            metadata_mismatches.push(format!("{name}: registered meta fields mismatch"));
        }
        let legacy = registry.shader_module(&legacy_name);
        if !legacy.is_some_and(|legacy| std::ptr::eq(canonical, legacy))
            || registry.kind(&name) != Some(ModuleKind::Shader)
            || registry.kind(&legacy_name) != Some(ModuleKind::Shader)
        {
            alias_mismatches.push(format!("{legacy_name}: does not resolve to {name}"));
        }
    }
    for (suffix, _) in ISF_TRANSITIONS {
        let name = format!("Plane9 {suffix}");
        let legacy_name = format!("Point10 {suffix}");
        let Some(canonical) = registry.isf_definition(&name) else {
            metadata_mismatches.push(format!("{name}: missing"));
            alias_mismatches.push(format!("{legacy_name}: canonical module missing"));
            continue;
        };
        if canonical.meta.author.as_deref() != Some("Plane9 (Joakim Dahl)") {
            metadata_mismatches.push(format!("{name}: author {:?}", canonical.meta.author));
        }
        let fields = registry.meta_fields(&name);
        if !fields.is_some_and(|fields| {
            fields.get("name").and_then(Value::as_str) == Some(name.as_str())
                && fields.get("author").and_then(Value::as_str) == Some("Plane9 (Joakim Dahl)")
        }) {
            metadata_mismatches.push(format!("{name}: registered meta fields mismatch"));
        }
        let legacy = registry.isf_definition(&legacy_name);
        if !legacy.is_some_and(|legacy| std::ptr::eq(canonical, legacy))
            || registry.kind(&name) != Some(ModuleKind::Isf)
            || registry.kind(&legacy_name) != Some(ModuleKind::Isf)
        {
            alias_mismatches.push(format!("{legacy_name}: does not resolve to {name}"));
        }
    }
    for suffix in POINT10_SHADER_EFFECTS {
        let name = format!("Point10 {suffix}");
        if !registry
            .shader_module(&name)
            .is_some_and(|module| module.meta_author == "Point10")
        {
            metadata_mismatches.push(format!("{name}: missing or wrong author"));
        }
        if registry
            .shader_module(&format!("Plane9 {suffix}"))
            .is_some()
        {
            metadata_mismatches.push(format!("Plane9 {suffix}: must not be registered"));
        }
    }
    check!(
        metadata_mismatches.is_empty(),
        "B: {} metadata mismatches: {}",
        metadata_mismatches.len(),
        summarize(&metadata_mismatches)
    );

    // C — the shared JSON rewrite covers nested saved documents, and both preset
    // restore and Scene import persist the canonical spelling for all 52 names.
    let mut nested_legacy = json!({"scene": {"modules": {"$moduleName": legacy_names}}});
    modv_renderers::point10::rename_legacy_scene_names(&mut nested_legacy);
    check!(
        nested_legacy["scene"]["modules"]["$moduleName"] == json!(canonical_names),
        "C: nested legacy references were not all rewritten to canonical Plane9 names"
    );

    let legacy_document = || {
        let mut modules = serde_json::Map::new();
        let mut module_ids = Vec::new();
        for (index, legacy_name) in legacy_names.iter().enumerate() {
            let id = format!("plane9-effect-{index}");
            module_ids.push(id.clone());
            modules.insert(
                id.clone(),
                json!({
                    "$id": id,
                    "$moduleName": legacy_name,
                    "meta": {"name": legacy_name},
                    "props": {},
                    "data": {},
                    "$props": {}
                }),
            );
        }
        json!({
            "id": "scene-legacy-plane9-effects",
            "name": "Legacy Plane9 effects",
            "groups": [{
                "id": "group-legacy-plane9-effects",
                "name": "Legacy",
                "hidden": false,
                "modules": module_ids,
                "enabled": 1,
                "alpha": 1,
                "inherit": true,
                "inheritFrom": -1,
                "pipeline": false,
                "clearing": false,
                "compositeOperation": "normal"
            }],
            "modules": Value::Object(modules),
            "inputs": {},
            "expressions": {},
            "tweens": [],
            "midi": {}
        })
    };

    let mut load_mismatches = Vec::new();
    let mut preset_store = Store::with_state_path_and_isf(None, Some(&corpus));
    match preset_store.restore(&legacy_document()) {
        Ok(()) => {
            for (index, canonical_name) in canonical_names.iter().enumerate() {
                let id = format!("plane9-effect-{index}");
                match preset_store.state().modules.get(&id) {
                    Some(module) if module.module_name == *canonical_name => {}
                    Some(module) => load_mismatches.push(format!(
                        "preset {id}: kept {:?}, expected {canonical_name:?}",
                        module.module_name
                    )),
                    None => load_mismatches.push(format!("preset {id}: missing")),
                }
            }
        }
        Err(error) => load_mismatches.push(format!("preset restore failed: {error:?}")),
    }

    let mut scene_store = Store::with_state_path_and_isf(None, Some(&corpus));
    let scene_params = json!({"scene": legacy_document(), "persist": false});
    match scene_store.load_scene_from_json(scene_params.as_object().expect("Scene params")) {
        Ok(imported) => {
            for (index, canonical_name) in canonical_names.iter().enumerate() {
                let id = format!("plane9-effect-{index}");
                let imported_name = imported["modules"][id.as_str()]["$moduleName"]
                    .as_str()
                    .unwrap_or_default();
                if imported_name != canonical_name {
                    load_mismatches.push(format!(
                        "Scene {id}: imported {imported_name:?}, expected {canonical_name:?}"
                    ));
                }
            }

            // Live Module ids are intentionally fresh on Scene import. Find the
            // stable Group and compare its live Modules in draw order instead of
            // assuming the live ids still match the stored document's ids.
            let live_names: Vec<&str> = scene_store
                .state()
                .groups
                .iter()
                .find(|group| group.name == "Legacy")
                .map(|group| {
                    group
                        .modules
                        .iter()
                        .filter_map(|id| scene_store.state().modules.get(id))
                        .map(|module| module.module_name.as_str())
                        .collect()
                })
                .unwrap_or_default();
            let expected_live_names: Vec<&str> =
                canonical_names.iter().map(String::as_str).collect();
            if live_names != expected_live_names {
                load_mismatches.push(format!(
                    "Scene Legacy: live {live_names:?}, expected {canonical_names:?}"
                ));
            }
        }
        Err(error) => load_mismatches.push(format!("Scene import failed: {error:?}")),
    }
    check!(
        alias_mismatches.is_empty(),
        "C: {} of 52 registry aliases failed: {}",
        alias_mismatches.len(),
        summarize(&alias_mismatches)
    );
    check!(
        load_mismatches.is_empty(),
        "C: {} saved-document migrations failed: {}",
        load_mismatches.len(),
        summarize(&load_mismatches)
    );

    // D — the info panel reports the audited upstream version for every renamed
    // effect, using the same exact label format as the Plane9 scenes.
    let mut licence_mismatches = Vec::new();
    for (suffix, version) in &renamed {
        let name = format!("Plane9 {suffix}");
        let module_type = if SHADER_EFFECTS
            .iter()
            .any(|(shader_suffix, _)| shader_suffix == suffix)
        {
            "shader"
        } else {
            "isf"
        };
        let expected =
            format!("CC BY-NC-SA {version} (Plane9 / Joakim Dahl; see THIRD_PARTY_NOTICES.md)");
        let licence = module_license(&ModuleMetaView {
            name: name.clone(),
            module_type: module_type.to_string(),
            kind: module_type.to_string(),
            ..ModuleMetaView::default()
        });
        if licence != expected {
            licence_mismatches.push(format!("{name}: got {licence:?}, expected {expected:?}"));
        }
    }
    check!(
        licence_mismatches.is_empty(),
        "D: {} licence mismatches: {}",
        licence_mismatches.len(),
        summarize(&licence_mismatches)
    );

    // E — every effect has a provenance row with the exact licence version and
    // source path. The two effects with additional upstream credits keep them.
    let notices = notices_section("Point10 scenes");
    let mut notice_mismatches = Vec::new();
    for (suffix, version) in &renamed {
        let name = format!("Plane9 {suffix}");
        let rows: Vec<&str> = notices
            .lines()
            .filter(|line| line.starts_with(&format!("| {suffix} |")))
            .collect();
        let source = if SHADER_EFFECTS
            .iter()
            .any(|(shader_suffix, _)| shader_suffix == suffix)
        {
            "crates/modv-renderers/src/shader/modules/point10_effects.rs".to_string()
        } else {
            format!("resources/isf/{name}.fs")
        };
        match rows.as_slice() {
            [row] if row.contains(&format!("CC BY-NC-SA {version}")) && row.contains(&source) => {}
            [row] => notice_mismatches.push(format!("{name}: wrong provenance row {row:?}")),
            [] => notice_mismatches.push(format!("{name}: no provenance row")),
            _ => notice_mismatches.push(format!("{name}: duplicate provenance rows")),
        }
    }
    let crt_row = notices
        .lines()
        .find(|line| line.starts_with("| Crt |"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    check!(
        crt_row.contains("timothy lottes") && crt_row.contains("public domain"),
        "E: Crt must retain Timothy Lottes' public-domain credit"
    );
    let cubic_row = notices
        .lines()
        .find(|line| line.starts_with("| Cubic Lens Distortion |"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    check!(
        cubic_row.contains("codepen"),
        "E: Cubic Lens Distortion must retain its CodePen credit"
    );
    check!(
        notice_mismatches.is_empty(),
        "E: {} notices provenance mismatches: {}",
        notice_mismatches.len(),
        summarize(&notice_mismatches)
    );

    // F — the ISF files themselves carry the new name/credit, the old filenames
    // are gone, and the checked-in translation baseline still matches the corpus.
    let mut source_mismatches = Vec::new();
    for (suffix, _) in ISF_TRANSITIONS {
        let name = format!("Plane9 {suffix}");
        let new_path = corpus.join(format!("{name}.fs"));
        let old_path = corpus.join(format!("Point10 {suffix}.fs"));
        if !new_path.is_file() {
            source_mismatches.push(format!("{name}: new ISF file missing"));
        }
        if old_path.exists() {
            source_mismatches.push(format!("Point10 {suffix}: old ISF file still exists"));
        }
        match fs::read_to_string(&new_path) {
            Ok(source) => match modv_renderers::isf::parse_isf(&source, None) {
                Ok(document) => {
                    if document.metadata.get("NAME").and_then(Value::as_str) != Some(name.as_str())
                    {
                        source_mismatches.push(format!("{name}: in-file NAME mismatch"));
                    }
                    if document.metadata.get("CREDIT").and_then(Value::as_str)
                        != Some("Plane9 (Joakim Dahl)")
                    {
                        source_mismatches.push(format!("{name}: in-file CREDIT mismatch"));
                    }
                }
                Err(error) => {
                    source_mismatches.push(format!("{name}: ISF header does not parse: {error}"))
                }
            },
            Err(_error) if !new_path.exists() => {}
            Err(error) => source_mismatches.push(format!("{name}: unreadable: {error}")),
        }
    }
    check!(
        source_mismatches.is_empty(),
        "F: {} ISF source mismatches: {}",
        source_mismatches.len(),
        summarize(&source_mismatches)
    );

    let baseline = modv_renderers::isf::translate::CorpusBaseline::from_json(
        modv_renderers::isf::translate::CORPUS_BASELINE,
    )
    .expect("the checked-in ISF corpus baseline parses");
    check!(
        baseline.total == 402,
        "F: baseline total is {}, expected 402",
        baseline.total
    );
    let baseline_names: BTreeSet<&str> = baseline
        .translated
        .iter()
        .chain(baseline.overridden.iter())
        .chain(baseline.failed.iter())
        .map(String::as_str)
        .collect();
    let missing_baseline_names: Vec<String> = ISF_TRANSITIONS
        .iter()
        .map(|(suffix, _)| format!("Plane9 {suffix}"))
        .filter(|name| {
            !baseline
                .translated
                .iter()
                .any(|translated| translated == name)
        })
        .collect();
    let stale_baseline_names: Vec<String> = ISF_TRANSITIONS
        .iter()
        .map(|(suffix, _)| format!("Point10 {suffix}"))
        .filter(|name| baseline_names.contains(name.as_str()))
        .collect();
    check!(
        missing_baseline_names.is_empty() && stale_baseline_names.is_empty(),
        "F: baseline is missing {} canonical transitions and retains {} legacy transitions",
        missing_baseline_names.len(),
        stale_baseline_names.len()
    );
    let report = modv_renderers::isf::translate::corpus_report(&corpus);
    let differences = baseline.differences(&report);
    check!(
        differences.is_empty(),
        "F: corpus baseline no longer validates: {}",
        differences.join("; ")
    );

    // G — the generator and both checked-in catalogue projections use the same
    // canonical registry names and unchanged shader/ISF counts.
    let generated = build_document(&corpus);
    let (metadata, previews) = split_documents(&generated);
    let committed_metadata: Value =
        serde_json::from_str(include_str!("../../../docs/modules/modules.json"))
            .expect("the committed module catalogue is valid JSON");
    let committed_previews: Value =
        serde_json::from_str(include_str!("../../../docs/modules/previews.json"))
            .expect("the committed preview catalogue is valid JSON");
    check!(
        metadata == committed_metadata,
        "G: docs/modules/modules.json does not match build_document()"
    );
    check!(
        previews == committed_previews,
        "G: docs/modules/previews.json does not match split_documents(build_document())"
    );
    check!(
        generated["counts"]["shader"].as_u64() == Some(36)
            && generated["counts"]["isf"].as_u64() == Some(402),
        "G: generated catalogue shader/ISF counts changed"
    );

    let modules = generated["modules"]
        .as_array()
        .expect("catalogue modules array");
    let catalogue_names: BTreeSet<String> = modules
        .iter()
        .filter_map(|module| module["name"].as_str().map(str::to_string))
        .collect();
    check!(
        catalogue_names == unique_registered_names,
        "G: generated catalogue names differ from the registry"
    );
    let mut catalogue_mismatches = Vec::new();
    for (suffix, _) in &renamed {
        let name = format!("Plane9 {suffix}");
        let legacy_name = format!("Point10 {suffix}");
        let matches: Vec<_> = modules
            .iter()
            .filter(|module| module["name"] == name)
            .collect();
        if matches.len() != 1 {
            catalogue_mismatches.push(format!("{name}: found {} entries", matches.len()));
        } else if matches[0]["author"] != "Plane9 (Joakim Dahl)" {
            catalogue_mismatches.push(format!("{name}: wrong catalogue author"));
        }
        if modules.iter().any(|module| module["name"] == legacy_name) {
            catalogue_mismatches.push(format!("{legacy_name}: stale catalogue entry"));
        }
    }
    check!(
        catalogue_mismatches.is_empty(),
        "G: {} generated catalogue mismatches: {}",
        catalogue_mismatches.len(),
        summarize(&catalogue_mismatches)
    );

    // The parity manifest is historical, like the Plane9 scene fixture: retain
    // every old spelling so alias-normalising registry guards can keep using it.
    let manifest: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/module-parity.json"))
            .expect("the historical parity manifest is valid JSON");
    let mut manifest_mismatches = Vec::new();
    for (suffix, _) in SHADER_EFFECTS {
        let old_name = format!("Point10 {suffix}");
        let new_name = format!("Plane9 {suffix}");
        let names = manifest["shader"]
            .as_array()
            .expect("shader manifest array");
        if !names
            .iter()
            .any(|name| name.as_str() == Some(old_name.as_str()))
        {
            manifest_mismatches.push(format!("shader manifest lost {old_name}"));
        }
        if names
            .iter()
            .any(|name| name.as_str() == Some(new_name.as_str()))
        {
            manifest_mismatches.push(format!("shader manifest rewrote {new_name}"));
        }
    }
    for (suffix, _) in ISF_TRANSITIONS {
        let old_name = format!("Point10 {suffix}");
        let new_name = format!("Plane9 {suffix}");
        let names = manifest["isf"].as_array().expect("ISF manifest array");
        if !names
            .iter()
            .any(|name| name.as_str() == Some(old_name.as_str()))
        {
            manifest_mismatches.push(format!("ISF manifest lost {old_name}"));
        }
        if names
            .iter()
            .any(|name| name.as_str() == Some(new_name.as_str()))
        {
            manifest_mismatches.push(format!("ISF manifest rewrote {new_name}"));
        }
    }
    check!(
        manifest_mismatches.is_empty(),
        "G: {} historical parity manifest mismatches: {}",
        manifest_mismatches.len(),
        summarize(&manifest_mismatches)
    );

    assert!(
        failures.is_empty(),
        "Plane9 effect rename contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
