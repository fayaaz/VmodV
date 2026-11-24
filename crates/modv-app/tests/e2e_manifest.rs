//! Guards the Electron → Rust e2e parity manifest after the port.
//!
//! `tests/e2e-rust/parity.json` is the inventory the port worked from. This
//! test survives the Electron suite (it embeds the JSON at compile time), so
//! the manifest cannot quietly regain an unported `port-*` bucket, lose a
//! waiver reason, or name a Rust test function that no longer exists.
//!
//! Run with:
//!
//! ```text
//! make engine-test ARGS="-p modv-app --test e2e_manifest"
//! ```

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const MANIFEST: &str = include_str!("../../../tests/e2e-rust/parity.json");
const EXPECTED_TESTS: usize = 146;
const EXPECTED_COVERED: usize = 136;
const EXPECTED_WAIVED: usize = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root resolves")
}

fn assert_reference_exists(root: &Path, reference: &str, spec: &str, test: &str) {
    let (path, symbol) = reference.rsplit_once(':').unwrap_or_else(|| {
        panic!("rust reference `{reference}` for {spec}: {test} has no `path:symbol` form")
    });
    let full = root.join(path);
    assert!(
        full.is_file(),
        "rust reference file `{path}` for {spec}: {test} is missing"
    );
    if path.ends_with(".rs") {
        let source = fs::read_to_string(&full)
            .unwrap_or_else(|error| panic!("cannot read `{path}`: {error}"));
        assert!(
            source.contains(&format!("fn {symbol}(")),
            "rust test `{symbol}` for {spec}: {test} is not defined in `{path}`"
        );
    }
}

#[test]
fn parity_manifest_is_final_and_its_rust_references_exist() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("parity.json parses");
    let specs = manifest["specs"].as_array().expect("specs array");
    let root = repo_root();

    let mut total = 0usize;
    let mut covered = 0usize;
    let mut waived = 0usize;
    let mut files = Vec::new();
    let mut keys = HashSet::new();

    for spec in specs {
        let file = spec["file"].as_str().expect("spec file");
        files.push(file.to_string());
        let tests = spec["tests"].as_array().expect("tests array");

        for test in tests {
            let name = test["name"].as_str().expect("test name");
            let status = test["status"].as_str().expect("test status");
            let key = format!("{file}\u{0}{name}");
            assert!(keys.insert(key), "duplicate manifest entry: {file}: {name}");
            total += 1;

            match status {
                "covered" => {
                    covered += 1;
                    let references = test["rust"]
                        .as_array()
                        .filter(|references| !references.is_empty())
                        .unwrap_or_else(|| {
                            panic!("covered entry without a Rust reference: {file}: {name}")
                        });
                    for reference in references {
                        let reference = reference.as_str().expect("rust reference string");
                        assert_reference_exists(&root, reference, file, name);
                    }
                }
                "waived" => {
                    waived += 1;
                    assert!(
                        !test["notes"].as_str().unwrap_or("").trim().is_empty(),
                        "waived entry without a concrete reason: {file}: {name}"
                    );
                }
                other => panic!("unknown or unported status `{other}`: {file}: {name}"),
            }
        }
    }

    assert_eq!(
        total, EXPECTED_TESTS,
        "the manifest must hold every executed Electron test"
    );
    assert_eq!(
        covered, EXPECTED_COVERED,
        "every non-waived Electron test must be covered"
    );
    assert_eq!(waived, EXPECTED_WAIVED, "waiver count");
    assert_eq!(covered + waived, total);

    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted, "spec files must stay sorted");
    assert_eq!(
        files.iter().collect::<HashSet<_>>().len(),
        files.len(),
        "a spec file is listed more than once"
    );
}
