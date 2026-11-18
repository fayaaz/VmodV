//! Guard the third-party notices against the Rust-only tree.
//!
//! This is the Rust replacement for `tests/unit/third-party-notices.test.js`.
//! It keeps the two properties that matter after the Electron tree is gone:
//!
//! * every registry crate in `crates/Cargo.lock` has a row with a valid SPDX
//!   licence and a plausible upstream link, and every row still resolves;
//! * every in-tree file that carries an NC-SA marker (the Rust ports under
//!   `crates/`, the ISF corpus and the bundled Point10 assets) is named in
//!   `THIRD_PARTY_NOTICES.md`, and its `"CREDIT"` author is credited.
//!
//! It also pins the specific attribution corrections the JS guard used to
//! cover (st_Ms2SD1, spherical-shader-tut, Perlin Transition, Helix Eye,
//! Traced Tunnel, the PCB texture and Around Plus) and the README licence
//! badge, so those do not regress when the notices are edited.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test notices"
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");
const CARGO_LOCK: &str = include_str!("../../Cargo.lock");
const README: &str = include_str!("../../../README.md");

const NOTICES_SECTION: &str = "Rust engine dependencies";

const WORKSPACE_CRATES: &[&str] = &[
    "modv-app",
    "modv-engine",
    "modv-ipc",
    "modv-media",
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
    if let Some(package) = current {
        if !package.name.is_empty() {
            packages.push(package);
        }
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
        {
            if let Some(bullet) = bullets.last_mut() {
                bullet.push(' ');
                bullet.push_str(line);
            }
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
        // This guard names the markers it searches for.
        if path.ends_with("crates/modv-app/tests/notices.rs") {
            continue;
        }
        let text_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "rs" | "wgsl" | "fs" | "vs" | "frag" | "json" | "md" | "txt" | "svg"
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

fn nc_sa_sources(root: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for directory in ["crates", "resources/isf", "resources/point10"] {
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
    if lower.starts_with("port of ") {
        if let Some(index) = lower.find(" by ") {
            let after_by = &raw[index + " by ".len()..];
            let end = after_by.find('(').unwrap_or(after_by.len());
            return Some(after_by[..end].trim().to_string());
        }
    }

    let raw = raw
        .strip_prefix("by ")
        .or_else(|| raw.strip_prefix("By "))
        .unwrap_or(raw);
    Some(raw.trim().to_string())
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

    assert!(NOTICES.contains("pcb.jpg"), "PCB texture is unnamed");
    assert!(NOTICES.contains("Miguel Alba"), "PCB author is uncredited");

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
}

#[test]
fn notices_no_longer_reference_the_deleted_electron_tree() {
    assert!(
        !NOTICES.contains("src/renderer/"),
        "THIRD_PARTY_NOTICES.md still names a deleted src/renderer path"
    );
}
