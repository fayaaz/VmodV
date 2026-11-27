//! Generator for the GitHub Pages module catalogue.
//!
//! The site must never drift from the app's own registry, so this module
//! builds the catalogue from `ModuleRegistry` (names, metadata, props) and
//! the ISF translator (the exact WGSL the compositor would compile). The
//! output is a single JSON document checked into `docs/modules/`, which the
//! static Pages site reads; no build step runs on the Pages side.
//!
//! Live browser previews cover the 33 WGSL shader modules and the 402 ISF
//! shaders. The 2D canvas modules and Point10 scenes have no portable WGSL
//! representation, so they are catalogued with full metadata but no preview.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::module_registry::{ModuleKind, ModuleRegistry};
use modv_renderers::isf::source::{
    UniformKind, UniformSource, texture_bindings, uniform_layout, uniform_members,
};
use modv_renderers::isf::translate::translate_fragment;
use modv_renderers::isf::{IsfPropType, runtime};

/// Catalogue schema version. Bump when the browser renderer needs a new
/// field shape.
pub const SCHEMA: u32 = 1;

/// Build the catalogue document for the corpus at `isf_dir`.
///
/// Pure and deterministic: the same corpus always yields the same JSON, so
/// CI can regenerate it and diff the checked-in copy.
pub fn build_document(isf_dir: &Path) -> Value {
    let registry = ModuleRegistry::build(Some(isf_dir));
    let mut modules = Vec::new();

    for module in registry.shader_modules() {
        let mut entry = base_entry(&module.name, "shader");
        entry["author"] = json!(module.meta_author);
        entry["version"] = json!(module.meta_version);
        entry["category"] = json!(module.category.as_deref().unwrap_or("effects"));
        entry["audioFeatures"] = json!(module.audio_features);
        entry["props"] = Value::Array(
            module
                .props
                .iter()
                .map(|prop| {
                    json!({
                        "name": prop.name,
                        "label": prop.label,
                        "type": prop_kind(prop.kind),
                        "default": prop.default,
                        "min": prop.min,
                        "max": prop.max,
                        "step": prop.step,
                    })
                })
                .collect(),
        );
        // The native pipeline prepends the full-screen-triangle vertex stage;
        // mirror that here so the browser gets a self-contained WGSL module.
        let wgsl = format!(
            "{}\n{}",
            modv_renderers::shader::default_shader::DEFAULT_VERTEX,
            module.fragment_wgsl
        );
        entry["preview"] = json!({
            "kind": "shader",
            "wgsl": wgsl,
            "sharedUniformBytes": 160,
        });
        modules.push(entry);
    }

    for definition in registry.two_d_definitions() {
        let meta = &definition.meta;
        let mut entry = base_entry(meta.name, "2d");
        entry["author"] = json!(meta.author);
        entry["version"] = json!(meta.version);
        entry["category"] = json!(meta.category.unwrap_or("2d"));
        entry["audioFeatures"] = json!(meta.audio_features);
        entry["props"] = Value::Array(
            definition
                .props
                .iter()
                .map(|prop| {
                    json!({
                        "name": prop.name,
                        "label": prop.label.unwrap_or(prop.name),
                        "type": prop.prop_type,
                        "default": prop.default,
                        "min": prop.min,
                        "max": prop.max,
                    })
                })
                .collect(),
        );
        modules.push(entry);
    }

    for definition in registry.point10_definitions() {
        let meta = &definition.meta;
        let mut entry = base_entry(meta.name, "point10");
        entry["author"] = json!(meta.author);
        entry["version"] = json!(meta.version);
        entry["category"] = json!(meta.category);
        entry["audioFeatures"] = json!(meta.audio_features);
        entry["props"] = Value::Array(
            definition
                .props
                .iter()
                .map(|prop| {
                    json!({
                        "name": prop.name,
                        "label": prop.label,
                        "type": scene_prop_kind(prop.kind),
                        "default": prop.default,
                        "min": prop.min,
                        "max": prop.max,
                    })
                })
                .collect(),
        );
        modules.push(entry);
    }

    for definition in registry.isf_definitions() {
        let doc = &definition.document;
        let mut entry = base_entry(&definition.name, "isf");
        entry["author"] = json!(definition.meta.author);
        entry["version"] = json!(definition.meta.version);
        entry["description"] = json!(definition.meta.description);
        entry["category"] = json!(
            definition
                .meta
                .category
                .clone()
                .unwrap_or_else(|| doc.filter_type().to_string())
        );
        entry["audioFeatures"] = json!(if doc.has_audio() {
            vec!["audio"]
        } else {
            vec![]
        });
        entry["source"] = json!(format!("resources/isf/{}.fs", definition.name));
        entry["passes"] = json!(doc.passes.len());
        entry["props"] = Value::Array(
            definition
                .props
                .iter()
                .map(|(name, prop)| isf_prop(name, prop))
                .collect(),
        );

        // The browser runtime draws pass 0 into the canvas with dummy input
        // textures; multi-pass/persistent shaders still show their main
        // effect, while a faithful replay stays in the native compositor.
        let translated = translate_fragment(doc, 0);
        if let Some(wgsl) = translated.ok().and_then(|shader| shader.wgsl) {
            let layout = uniform_layout(&uniform_members(doc));
            let members = layout
                .members
                .iter()
                .map(|member| {
                    json!({
                        "name": member.name,
                        "kind": uniform_kind(member.kind),
                        "source": uniform_source(&member.source),
                        "offset": member.offset,
                    })
                })
                .collect::<Vec<_>>();
            let bindings = texture_bindings(doc)
                .iter()
                .map(|binding| {
                    json!({
                        "name": binding.name,
                        "texture": binding.texture_binding,
                        "sampler": binding.sampler_binding,
                    })
                })
                .collect::<Vec<_>>();
            let bottom_left = bindings.is_empty();
            entry["preview"] = json!({
                "kind": "isf",
                "wgsl": wgsl,
                "vertex": if bottom_left { runtime::BOTTOM_LEFT_VERTEX_WGSL } else { runtime::DEFAULT_VERTEX_WGSL },
                "uniformLayout": {"size": layout.size, "members": members},
                "textureBindings": bindings,
                "passes": doc.passes.len(),
                "simplified": doc.passes.len() > 1 || doc.passes.iter().any(|pass| pass.persistent),
            });
        }
        modules.push(entry);
    }

    disambiguate_ids(&mut modules);

    modules.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .cmp(&b["name"].as_str().unwrap_or_default().to_ascii_lowercase())
    });

    let counts = [
        ("2d", ModuleKind::TwoD),
        ("point10", ModuleKind::Point10),
        ("shader", ModuleKind::Shader),
        ("isf", ModuleKind::Isf),
    ]
    .into_iter()
    .map(|(key, kind)| {
        (
            key.to_string(),
            Value::from(
                modules
                    .iter()
                    .filter(|module| module["kind"] == kind.as_str())
                    .count(),
            ),
        )
    })
    .collect::<Map<_, _>>();
    let total: usize = modules.len();
    // Keep `total` first so the front page can read it without summing.
    let mut ordered_counts = Map::new();
    ordered_counts.insert("total".to_string(), Value::from(total));
    for (key, value) in counts {
        ordered_counts.insert(key, value);
    }

    json!({
        "schema": SCHEMA,
        "source": "ModuleRegistry + modv_renderers::isf (see tools/gen-module-docs.sh)",
        "counts": ordered_counts,
        "modules": modules,
    })
}

/// Split a [`build_document`] into the two documents the static site loads.
///
/// `modules.json` is the small catalogue shown on page load; WGSL lives in
/// `previews.json`, fetched only when someone opens a live preview. Keeping
/// the shaders out of the initial payload makes the front page fast even with
/// 494 compilable modules. Both the generator and the catalogue guards use
/// this projection so they cannot disagree about which file holds what.
pub fn split_documents(document: &Value) -> (Value, Value) {
    let mut metadata = document.clone();
    let mut previews = Map::new();

    if let Some(modules) = metadata["modules"].as_array_mut() {
        for module in modules.iter_mut() {
            let id = module["id"].as_str().unwrap_or_default().to_string();
            let Some(preview) = module["preview"].as_object_mut() else {
                continue;
            };
            let mut shader = Map::new();
            for key in ["wgsl", "vertex", "uniformLayout", "textureBindings"] {
                if let Some(value) = preview.remove(key) {
                    shader.insert(key.to_string(), value);
                }
            }
            previews.insert(id, Value::Object(shader));
        }
    }

    (metadata, Value::Object(previews))
}

/// Generate and write the metadata/preview pair the static site loads.
pub fn write_documents(isf_dir: &Path, output_dir: &Path) -> Result<(), std::io::Error> {
    let (metadata, previews) = split_documents(&build_document(isf_dir));

    fs::create_dir_all(output_dir)?;
    write_json(&output_dir.join("modules.json"), &metadata)?;
    write_json(&output_dir.join("previews.json"), &previews)
}

/// Write one pretty-printed, newline-terminated JSON document.
fn write_json(path: &Path, value: &Value) -> Result<(), std::io::Error> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    text.push('\n');
    fs::write(path, text)
}

fn base_entry(name: &str, kind: &str) -> Value {
    let mut entry = Map::new();
    entry.insert("id".into(), json!(slug(name)));
    entry.insert("name".into(), json!(name));
    entry.insert("kind".into(), json!(kind));
    entry.insert("category".into(), json!(kind));
    entry.insert("author".into(), Value::Null);
    entry.insert("version".into(), Value::Null);
    entry.insert("description".into(), Value::Null);
    entry.insert("audioFeatures".into(), json!([]));
    entry.insert("props".into(), json!([]));
    entry.insert("source".into(), Value::Null);
    entry.insert("preview".into(), Value::Null);
    Value::Object(entry)
}

/// The ISF corpus has case-different files (`Bloom.fs` and `bloom.fs`) that
/// are distinct modules but share a lowercase slug. Keep both URLs stable by
/// suffixing every member of a colliding group with a short name hash.
fn disambiguate_ids(modules: &mut [Value]) {
    use std::collections::HashMap;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for module in modules.iter() {
        *counts
            .entry(module["id"].as_str().unwrap_or_default().to_string())
            .or_default() += 1;
    }
    for module in modules.iter_mut() {
        let id = module["id"].as_str().unwrap_or_default().to_string();
        if counts.get(&id).copied().unwrap_or(0) > 1 {
            let hash = fnv1a32(module["name"].as_str().unwrap_or(&id));
            module["id"] = json!(format!("{id}-{hash:04x}"));
        }
    }
}

fn fnv1a32(text: &str) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// URL-safe, stable identifier derived from the registration name.
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

fn prop_kind(kind: modv_renderers::shader::uniforms::PropKind) -> &'static str {
    use modv_renderers::shader::uniforms::PropKind;
    match kind {
        PropKind::Float => "float",
        PropKind::Int => "int",
        PropKind::Bool => "bool",
        PropKind::Enum => "enum",
        PropKind::Vec2 => "vec2",
        PropKind::Vec3 => "vec3",
        PropKind::Vec4 => "vec4",
        PropKind::Color => "color",
    }
}

fn scene_prop_kind(kind: modv_renderers::point10::ScenePropKind) -> &'static str {
    use modv_renderers::point10::ScenePropKind;
    match kind {
        ScenePropKind::Float => "float",
        ScenePropKind::Bool => "bool",
    }
}

fn isf_prop(name: &str, prop: &modv_renderers::isf::IsfProp) -> Value {
    let kind = match prop.type_ {
        IsfPropType::Texture => "texture",
        _ => prop.type_.as_str(),
    };
    json!({
        "name": name,
        "label": prop.label,
        "type": kind,
        "default": prop.default,
        "min": prop.min,
        "max": prop.max,
        "step": prop.step,
    })
}

fn uniform_kind(kind: UniformKind) -> &'static str {
    match kind {
        UniformKind::Float => "float",
        UniformKind::Int => "int",
        UniformKind::Vec2 => "vec2",
        UniformKind::Vec4 => "vec4",
    }
}

fn uniform_source(source: &UniformSource) -> String {
    match source {
        UniformSource::PassIndex => "passIndex".into(),
        UniformSource::RenderSize => "renderSize".into(),
        UniformSource::Time => "time".into(),
        UniformSource::TimeDelta => "timeDelta".into(),
        UniformSource::FrameIndex => "frameIndex".into(),
        UniformSource::Date => "date".into(),
        UniformSource::Input(name) => format!("input:{name}"),
        UniformSource::ImageSize(name) => format!("imageSize:{name}"),
        UniformSource::ImageRect(name) => format!("imageRect:{name}"),
        UniformSource::ImageFlip(name) => format!("imageFlip:{name}"),
    }
}
