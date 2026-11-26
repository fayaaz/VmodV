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

use modv_renderers::point10::all_scenes;

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
                .strip_prefix("Point10 ")
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
