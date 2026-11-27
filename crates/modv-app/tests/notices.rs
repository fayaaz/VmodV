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
    // payload; they are not themselves NC-SA-licensed sources.
    for described in [
        "crates/modv-app/tests/appimage_bundle.rs",
        "crates/modv-app/tests/deb_package.rs",
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
        scene_names.len() == 47,
        "A: expected 47 scenes, got {}",
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
    if manifest_names.len() != 47 {
        manifest_mismatches.push(format!("expected 47 names, got {}", manifest_names.len()));
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
        scenes.len() == 47,
        "roster: expected 47 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "roster: duplicate names in {names:?}"
    );
    check!(
        modv_renderers::point10::PLANE9_SCENE_NAMES.len() == 29,
        "Plane9 alias roster: expected 29 names, got {}",
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
        registry.point10_definitions().len() == 47,
        "ModuleRegistry: expected 47 Point10 definitions, got {}",
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
        manifest["counts"]["point10"].as_u64() == Some(47),
        "manifest: point10 count is {:?}, expected 47",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(501),
        "manifest: total is {:?}, expected 501",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 47,
        "manifest: expected 47 Point10 names, got {}",
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
        generated["counts"]["point10"].as_u64() == Some(47),
        "catalogue: point10 count is {:?}, expected 47",
        generated["counts"]["point10"]
    );
    check!(
        generated["counts"]["total"].as_u64() == Some(501),
        "catalogue: total is {:?}, expected 501",
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
        scenes.len() == 47,
        "roster: expected 47 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "roster: duplicate names in {names:?}"
    );

    let aliases = modv_renderers::point10::PLANE9_SCENE_NAMES;
    let unique_aliases: BTreeSet<&str> = aliases.iter().copied().collect();
    check!(
        aliases.len() == 29,
        "Plane9 alias roster: expected 29 names, got {}",
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
        registry.point10_definitions().len() == 47,
        "ModuleRegistry: expected 47 Point10 definitions, got {}",
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
        manifest["counts"]["point10"].as_u64() == Some(47),
        "manifest: point10 count is {:?}, expected 47",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(501),
        "manifest: total is {:?}, expected 501",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 47,
        "manifest: expected 47 Point10 names, got {}",
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
        generated["counts"]["point10"].as_u64() == Some(47),
        "catalogue: point10 count is {:?}, expected 47",
        generated["counts"]["point10"]
    );
    check!(
        generated["counts"]["total"].as_u64() == Some(501),
        "catalogue: total is {:?}, expected 501",
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
