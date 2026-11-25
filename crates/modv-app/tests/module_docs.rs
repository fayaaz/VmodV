//! Guards for the generated GitHub Pages module catalogue.
//!
//! The catalogue is produced from the same `ModuleRegistry` the app uses, so
//! it must cover every registered module, carry real metadata/props, and ship
//! compilable WGSL for every shader/ISF module that gets a live browser
//! preview. These tests run without a GPU: translation is CPU-only.

use std::path::Path;

use modv_app::module_docs::{build_document, write_documents};

fn corpus() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/isf")
        .leak()
}

fn manifest() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/module-parity.json"))
        .expect("the parity manifest must be valid JSON")
}

#[test]
fn document_covers_every_registered_module_in_the_parity_manifest() {
    let doc = build_document(corpus());
    let manifest = manifest();
    let modules = doc["modules"].as_array().expect("modules array");

    assert_eq!(doc["schema"].as_u64(), Some(1));
    assert_eq!(
        doc["counts"]["total"].as_u64(),
        manifest["counts"]["total"].as_u64(),
        "the catalogue must publish its own total"
    );
    assert_eq!(
        modules.len(),
        manifest["counts"]["total"].as_u64().unwrap() as usize
    );

    for (key, kind) in [
        ("twoD", "2d"),
        ("point10", "point10"),
        ("shader", "shader"),
        ("isf", "isf"),
    ] {
        let expected = manifest["counts"][key].as_u64().unwrap() as usize;
        let actual = modules.iter().filter(|m| m["kind"] == kind).count();
        assert_eq!(actual, expected, "wrong {kind} count in the catalogue");
    }
}

#[test]
fn every_module_has_a_unique_id_and_a_real_name() {
    let doc = build_document(corpus());
    let modules = doc["modules"].as_array().unwrap();

    let mut ids = std::collections::HashSet::new();
    for module in modules {
        let id = module["id"].as_str().expect("module id");
        let name = module["name"].as_str().expect("module name");
        assert!(
            !id.is_empty() && !name.is_empty(),
            "empty id/name: {module}"
        );
        assert!(ids.insert(id.to_string()), "duplicate module id: {id}");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "id must be URL-safe: {id}"
        );
    }
}

#[test]
fn shader_and_isf_modules_ship_compilable_wgsl_previews() {
    let doc = build_document(corpus());
    let modules = doc["modules"].as_array().unwrap();

    let mut shader_previews = 0;
    let mut isf_previews = 0;
    for module in modules {
        match module["kind"].as_str().unwrap() {
            "shader" => {
                shader_previews += 1;
                let preview = &module["preview"];
                assert_eq!(
                    preview["kind"], "shader",
                    "{} has no shader preview",
                    module["name"]
                );
                let wgsl = preview["wgsl"].as_str().expect("shader WGSL");
                assert!(
                    wgsl.contains("@fragment"),
                    "shader WGSL missing fragment stage"
                );
                assert!(preview["sharedUniformBytes"].as_u64() == Some(160));
            }
            "isf" => {
                isf_previews += 1;
                let preview = &module["preview"];
                assert_eq!(
                    preview["kind"], "isf",
                    "{} has no ISF preview",
                    module["name"]
                );
                let wgsl = preview["wgsl"].as_str().expect("ISF WGSL");
                assert!(
                    wgsl.contains("fn main"),
                    "ISF WGSL missing fragment entry point"
                );
                let layout = &preview["uniformLayout"];
                let size = layout["size"].as_u64().expect("uniform size");
                assert!(
                    size >= 16 && size % 16 == 0,
                    "bad uniform size for {}",
                    module["name"]
                );
            }
            "2d" | "point10" => {
                assert!(
                    module["preview"].is_null(),
                    "2D/Point10 previews are engine-only"
                );
            }
            other => panic!("unexpected module kind: {other}"),
        }
    }
    assert_eq!(shader_previews, 36);
    assert_eq!(isf_previews, 396);
}

#[test]
fn props_and_metadata_are_included_for_gallery_cards() {
    let doc = build_document(corpus());
    let modules = doc["modules"].as_array().unwrap();

    let plasma = modules
        .iter()
        .find(|m| m["name"] == "Plasma")
        .expect("Plasma is a built-in shader");
    assert_eq!(plasma["kind"], "shader");
    assert!(plasma["props"].as_array().unwrap().len() >= 3);
    assert!(plasma["props"][0]["default"].is_number());

    let bloom = modules
        .iter()
        .find(|m| m["name"] == "Bloom" && m["kind"] == "isf")
        .expect("Bloom is in the ISF corpus");
    assert_eq!(bloom["source"], "resources/isf/Bloom.fs");
    assert_eq!(bloom["passes"].as_u64(), Some(7));
    assert!(
        bloom["props"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "inputImage")
    );
}

#[test]
fn written_pages_files_split_metadata_from_wgsl() {
    let dir = std::env::temp_dir().join(format!("modv-module-docs-{}", std::process::id()));
    write_documents(corpus(), &dir).expect("write the Pages data files");

    let catalog_text = std::fs::read_to_string(dir.join("modules.json")).unwrap();
    assert!(
        catalog_text.len() < 2_000_000,
        "the catalogue should stay small enough to load on the front page"
    );
    let catalog: serde_json::Value = serde_json::from_str(&catalog_text).unwrap();
    let previews_text = std::fs::read_to_string(dir.join("previews.json")).unwrap();
    let previews: serde_json::Value = serde_json::from_str(&previews_text).unwrap();

    for module in catalog["modules"].as_array().unwrap() {
        if module["preview"].is_null() {
            continue;
        }
        let id = module["id"].as_str().unwrap();
        assert_eq!(module["preview"]["wgsl"], serde_json::Value::Null);
        assert!(
            previews[id]["wgsl"].as_str().is_some(),
            "missing WGSL for {id}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn document_is_deterministic() {
    let first = build_document(corpus());
    let second = build_document(corpus());
    assert_eq!(
        first, second,
        "the catalogue must be byte-stable across runs"
    );
}
