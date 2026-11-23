//! Parser and filesystem discovery for user-authored WGSL + JSON modules.
//!
//! See BUGS.md item 5. [`parse_user_module`] is pure: it turns descriptor JSON
//! (plus an optional sibling WGSL source string) into a validated
//! [`UserModuleDescriptor`]. [`load_user_modules`] finds those descriptors in
//! user module directories and attaches the referenced WGSL source. No GPU or
//! registry work happens here.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use crate::shader::uniforms::PropKind;

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

/// Discover user modules in `dirs`, returning them in deterministic path order.
///
/// Every directory is scanned for `.json` descriptors (case-insensitively).
/// A descriptor either carries its WGSL inline (`wgslSource`) or names a
/// sibling file (`wgsl`) next to the descriptor. Hidden/temp files, unreadable
/// files, malformed descriptors and duplicate module names are skipped.
pub fn load_user_modules(dirs: &[PathBuf]) -> Vec<UserModuleDescriptor> {
    let mut paths = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        paths.extend(entries.filter_map(Result::ok).filter_map(|entry| {
            let path = entry.path();
            let is_json = path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
            (is_json && !is_ignored_name(&path)).then_some(path)
        }));
    }
    paths.sort();

    let mut seen = HashSet::new();
    let mut modules = Vec::new();
    for path in paths {
        let Ok(json) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        let sibling = sibling_source(&path, object);
        let Ok(descriptor) = descriptor_from_object(object, sibling.as_deref()) else {
            continue;
        };
        if seen.insert(descriptor.name.clone()) {
            modules.push(descriptor);
        }
    }
    modules
}

/// Hidden, backup and temporary file names are not user module descriptors.
fn is_ignored_name(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            name.starts_with('.')
                || name.starts_with('~')
                || name.ends_with('~')
                || name.ends_with(".tmp")
        })
}

/// Read the sibling file named by a descriptor's `wgsl` field, if any.
fn sibling_source(path: &Path, object: &Map<String, Value>) -> Option<String> {
    if object
        .get("wgslSource")
        .and_then(Value::as_str)
        .is_some_and(|source| !source.is_empty())
    {
        return None;
    }
    let name = object.get("wgsl")?.as_str()?;
    let relative = Path::new(name);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let source = fs::read_to_string(path.parent()?.join(relative)).ok()?;
    (!source.is_empty()).then_some(source)
}

/// Failures reported by the pure descriptor parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserModuleParseError {
    InvalidJson(String),
    DescriptorNotAnObject,
    MissingName,
    MissingWgslSource,
    InvalidPropType { name: String, found: String },
}

/// Parse a descriptor JSON document plus optional sibling WGSL source text.
///
/// Inline `wgslSource` is preferred when present; otherwise the caller-supplied
/// sibling source (the contents of the file named by `wgsl`) is attached.
pub fn parse_user_module(
    descriptor_json: &str,
    sibling_wgsl_source: Option<&str>,
) -> Result<UserModuleDescriptor, UserModuleParseError> {
    let value: Value = serde_json::from_str(descriptor_json)
        .map_err(|err| UserModuleParseError::InvalidJson(err.to_string()))?;
    let object = value
        .as_object()
        .ok_or(UserModuleParseError::DescriptorNotAnObject)?;
    descriptor_from_object(object, sibling_wgsl_source)
}

/// Build a descriptor from an already-parsed JSON object. Both the public
/// parser and filesystem discovery use this, so discovery parses each file
/// only once.
fn descriptor_from_object(
    object: &Map<String, Value>,
    sibling_wgsl_source: Option<&str>,
) -> Result<UserModuleDescriptor, UserModuleParseError> {
    let name = string_field(object, "name");
    if name.is_empty() {
        return Err(UserModuleParseError::MissingName);
    }

    let wgsl_source = match optional_string_field(object, "wgslSource") {
        Some(source) => source,
        None => sibling_wgsl_source.unwrap_or_default().to_string(),
    };
    if wgsl_source.is_empty() {
        return Err(UserModuleParseError::MissingWgslSource);
    }

    let mut props = Vec::new();
    if let Some(values) = object.get("props").and_then(Value::as_array) {
        for value in values {
            props.push(parse_prop(value)?);
        }
    }

    Ok(UserModuleDescriptor {
        name,
        author: string_field(object, "author"),
        version: non_empty_string_field(object, "version", "1.0.0"),
        description: string_field(object, "description"),
        category: optional_string_field(object, "category"),
        license: optional_string_field(object, "license"),
        wgsl_source,
        entry: non_empty_string_field(object, "entry", "fs_main"),
        audio: bool_field(object, "audio", false),
        preview_with_output: bool_field(object, "previewWithOutput", true),
        props,
    })
}

fn parse_prop(value: &Value) -> Result<UserModuleProp, UserModuleParseError> {
    let object = value.as_object();
    let field = |key: &str| object.and_then(|object| object.get(key));

    let name = field("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let found = field("type").and_then(Value::as_str).unwrap_or_default();
    let kind = match found {
        "float" => PropKind::Float,
        "int" => PropKind::Int,
        "bool" => PropKind::Bool,
        "enum" => PropKind::Enum,
        "vec2" => PropKind::Vec2,
        "vec3" => PropKind::Vec3,
        "vec4" => PropKind::Vec4,
        "color" => PropKind::Color,
        other => {
            return Err(UserModuleParseError::InvalidPropType {
                name,
                found: other.to_string(),
            });
        }
    };

    let label = field("label")
        .and_then(Value::as_str)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| name.clone());
    let number = |key: &str, default: f64| field(key).and_then(Value::as_f64).unwrap_or(default);

    Ok(UserModuleProp {
        name,
        label,
        kind,
        default: number("default", 0.0),
        min: number("min", 0.0),
        max: number("max", 1.0),
        step: number("step", 0.01),
    })
}

fn string_field(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn non_empty_string_field(object: &Map<String, Value>, key: &str, default: &str) -> String {
    let value = string_field(object, key);
    if value.is_empty() {
        default.to_string()
    } else {
        value
    }
}

fn optional_string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_field(object: &Map<String, Value>, key: &str, default: bool) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(default)
}
