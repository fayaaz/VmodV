//! Test for user-authored WGSL + JSON module descriptors.
//!
//! Scope (BUGS.md item 5, user-chosen design): users can drop a descriptor
//! plus a sibling WGSL file into `media/<profile>/module/` and get a
//! single-pass fragment module with ISF/builtin-style time + audio uniforms
//! and typed props. This test covers the **pure parser** only.

use std::fmt;

use modv_renderers::shader::uniforms::PropKind;
use modv_renderers::user_module::{
    parse_user_module, UserModuleDescriptor, UserModuleParseError, UserModuleProp,
};

fn check_eq<T: fmt::Debug + PartialEq>(
    label: &str,
    got: &T,
    expected: &T,
    failures: &mut Vec<String>,
) {
    if got != expected {
        failures.push(format!("{label}: got {got:?}, expected {expected:?}"));
    }
}

#[test]
fn user_module_descriptor_parses_with_defaults_and_clear_errors() {
    // A minimal, self-contained fragment module — the WGSL the sibling file
    // `scene.wgsl` would contain for the full descriptor below.
    const WGSL: &str = "@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> { return vec4<f32>(p.x / 100.0, 0.0, 0.0, 1.0); }";
    const INLINE_WGSL: &str =
        "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }";

    let full_json = r#"{
        "name": "My Scene",
        "author": "Alice",
        "version": "1.0.0",
        "description": "A generated scene",
        "category": "2d-scenes",
        "license": "MIT",
        "wgsl": "scene.wgsl",
        "entry": "fs_main",
        "audio": true,
        "previewWithOutput": false,
        "props": [
            {"name":"speed","label":"Speed","type":"float","default":1.0,"min":0.0,"max":4.0,"step":0.01},
            {"name":"enabled","type":"bool","default":1.0}
        ]
    }"#;

    let minimal_json = format!(r#"{{"name":"Tiny","wgslSource":"{INLINE_WGSL}"}}"#);
    let missing_name = format!(r#"{{"wgslSource":"{INLINE_WGSL}"}}"#);
    let missing_wgsl = r#"{"name":"No Source"}"#;
    let bad_prop_type = format!(
        r#"{{"name":"Bad","wgslSource":"{INLINE_WGSL}","props":[{{"name":"oops","type":"geometry"}}]}}"#
    );

    let mut failures = Vec::new();

    // 1. Full descriptor: every declared field survives, sibling WGSL is
    //    attached, props keep declaration order and are typed correctly.
    match parse_user_module(full_json, Some(WGSL)) {
        Ok(got) => {
            let expected = UserModuleDescriptor {
                name: "My Scene".to_string(),
                author: "Alice".to_string(),
                version: "1.0.0".to_string(),
                description: "A generated scene".to_string(),
                category: Some("2d-scenes".to_string()),
                license: Some("MIT".to_string()),
                wgsl_source: WGSL.to_string(),
                entry: "fs_main".to_string(),
                audio: true,
                preview_with_output: false,
                props: vec![
                    UserModuleProp {
                        name: "speed".to_string(),
                        label: "Speed".to_string(),
                        kind: PropKind::Float,
                        default: 1.0,
                        min: 0.0,
                        max: 4.0,
                        step: 0.01,
                    },
                    UserModuleProp {
                        name: "enabled".to_string(),
                        // Omitted `label` falls back to the prop name.
                        label: "enabled".to_string(),
                        kind: PropKind::Bool,
                        default: 1.0,
                        // Omitted numeric metadata gets sensible defaults.
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                    },
                ],
            };
            check_eq("full descriptor", &got, &expected, &mut failures);
        }
        other => failures.push(format!("full descriptor: expected Ok(...), got {other:?}")),
    }

    // 2. Minimal descriptor: inline `wgslSource` works and every optional
    //    field gets its documented default.
    match parse_user_module(&minimal_json, None) {
        Ok(got) => {
            let expected = UserModuleDescriptor {
                name: "Tiny".to_string(),
                author: String::new(),
                version: "1.0.0".to_string(),
                description: String::new(),
                category: None,
                license: None,
                wgsl_source: INLINE_WGSL.to_string(),
                entry: "fs_main".to_string(),
                audio: false,
                preview_with_output: true,
                props: Vec::new(),
            };
            check_eq(
                "minimal descriptor defaults",
                &got,
                &expected,
                &mut failures,
            );
        }
        other => failures.push(format!(
            "minimal descriptor: expected Ok(...), got {other:?}"
        )),
    }

    // 3. Required fields and prop types are rejected with clear errors — not
    //    silently defaulted or ignored.
    for (label, json, sibling, expected) in [
        (
            "missing name",
            missing_name.as_str(),
            Some(WGSL),
            UserModuleParseError::MissingName,
        ),
        (
            "missing WGSL source",
            missing_wgsl,
            None,
            UserModuleParseError::MissingWgslSource,
        ),
        (
            "invalid prop type",
            bad_prop_type.as_str(),
            None,
            UserModuleParseError::InvalidPropType {
                name: "oops".to_string(),
                found: "geometry".to_string(),
            },
        ),
    ] {
        match parse_user_module(json, sibling) {
            Err(got) if got == expected => {}
            other => failures.push(format!(
                "{label}: expected Err({expected:?}), got {other:?}"
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "user-module descriptor parser contract unmet:\n  {}",
        failures.join("\n  ")
    );
}
