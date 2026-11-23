//! RED-phase test for user-authored WGSL + JSON module descriptors.
//!
//! Scope (BUGS.md item 5, user-chosen design): users can drop a descriptor
//! plus a sibling WGSL file into `media/<profile>/module/` and get a
//! single-pass fragment module with ISF/builtin-style time + audio uniforms
//! and typed props. This test covers the **pure parser** only.
//!
//! # TEMPORARY RED SHIM — DELETE IN GREEN
//!
//! The production `modv_renderers::user_module` module does not exist yet.
//! The `red_shim` module below declares the API the GREEN implementation
//! must provide and stubs `parse_user_module` with a sentinel error so this
//! test compiles today and fails on an assertion (never a compile error).
//!
//! GREEN implementer: delete the `red_shim` module and its `use`, then add
//! `use modv_renderers::user_module::{parse_user_module, UserModuleDescriptor,
//! UserModuleParseError, UserModuleProp};`. Keep the assertions unchanged.

use std::fmt;

// ---------------------------------------------------------------------------
// BEGIN TEMPORARY RED SHIM (delete in GREEN, import the real API instead).
// ---------------------------------------------------------------------------
mod red_shim {
    use modv_renderers::shader::uniforms::PropKind;

    /// One user-declared prop, in declaration (= prop-buffer slot) order.
    #[derive(Clone, Debug, PartialEq)]
    pub struct UserModuleProp {
        pub name: String,
        pub label: String,
        pub kind: PropKind,
        pub default: f64,
        pub min: f64,
        pub max: f64,
        pub step: f64,
    }

    /// A fully parsed, ready-to-register user module definition.
    #[derive(Clone, Debug, PartialEq)]
    pub struct UserModuleDescriptor {
        pub name: String,
        pub author: String,
        pub version: String,
        pub description: String,
        pub category: Option<String>,
        pub license: Option<String>,
        pub wgsl_source: String,
        pub entry: String,
        pub audio: bool,
        pub preview_with_output: bool,
        pub props: Vec<UserModuleProp>,
    }

    /// Failures reported by the pure descriptor parser.
    // The real parser will construct every variant; the shim only returns
    // NotImplemented, so silence dead_code here (removed with the shim).
    #[allow(dead_code)]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum UserModuleParseError {
        InvalidJson(String),
        DescriptorNotAnObject,
        MissingName,
        MissingWgslSource,
        InvalidPropType {
            name: String,
            found: String,
        },
        /// Shim-only sentinel — remove in GREEN.
        NotImplemented,
    }

    /// Pure parser: JSON descriptor text plus optional sibling WGSL source.
    pub fn parse_user_module(
        _descriptor_json: &str,
        _sibling_wgsl_source: Option<&str>,
    ) -> Result<UserModuleDescriptor, UserModuleParseError> {
        Err(UserModuleParseError::NotImplemented)
    }
}
// ---------------------------------------------------------------------------
// END TEMPORARY RED SHIM.
// ---------------------------------------------------------------------------

use modv_renderers::shader::uniforms::PropKind;
use red_shim::{UserModuleDescriptor, UserModuleParseError, UserModuleProp, parse_user_module};

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
