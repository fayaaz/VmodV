//! MilkDrop preset → modV module metadata and prop synthesis (Layer 4).
//!
//! Mirrors the shape of [`crate::isf::props`] so the module registry can share
//! its prop-schema conversion between ISF and MilkDrop modules.
//!
//! # Metadata (`MilkMeta`)
//!
//! MilkDrop presets carry no first-class, always-populated metadata header the
//! way ISF does — the parser ([`crate::milkdrop::preset`]) recognises but
//! discards `author`/`desc`/`description` keys. Author and display name are
//! therefore recovered from [`MilkDocument::name`], which is either the
//! preset's `title`/`presetname` line or, failing that, the file stem passed to
//! [`crate::milkdrop::parse_milk`]. When that string follows the common
//! `Name - Author` convention, the two halves are split; otherwise the author
//! is the literal `"unknown"` (no person is ever invented).
//!
//! Category is always `"effects"` and `preview_with_output` is always `false`
//! — MilkDrop presets are self-contained effects that do not consume the
//! previous frame as a visible input texture.

#![forbid(unsafe_code)]

use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::isf::props::EnumMember;
use crate::milkdrop::MilkDocument;

/// Category assigned to every MilkDrop module.
const CATEGORY: &str = "effects";
/// Fallback author when a preset name has no `Name - Author` suffix; a person
/// is never invented.
const UNKNOWN_AUTHOR: &str = "unknown";
/// Separator recognised by the common `Name - Author` naming convention.
const AUTHOR_SEPARATOR: &str = " - ";

/// The prop type a synthesised MilkDrop prop becomes.
///
/// These mirror [`crate::isf::props::IsfPropType`] so the two renderer
/// families can share the registry's prop-schema conversion. The strings
/// returned by [`MilkPropType::as_str`] are the `dataTypes` keys the engine's
/// prop panels expect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MilkPropType {
    Float,
    Bool,
    Enum,
    Vec4,
    Vec2,
    Texture,
    Event,
}

impl MilkPropType {
    /// The `dataTypes` key the engine's prop panels expect for this type.
    pub fn as_str(self) -> &'static str {
        match self {
            MilkPropType::Float => "float",
            MilkPropType::Bool => "bool",
            MilkPropType::Enum => "enum",
            MilkPropType::Vec4 => "vec4",
            MilkPropType::Vec2 => "vec2",
            MilkPropType::Texture => "texture",
            MilkPropType::Event => "event",
        }
    }
}

/// One synthesised MilkDrop module prop. Field-for-field shape match with
/// [`crate::isf::props::IsfProp`].
#[derive(Clone, Debug, PartialEq)]
pub struct MilkProp {
    /// Prop type.
    pub type_: MilkPropType,
    /// Panel label.
    pub label: String,
    /// Initial value, when one is synthesised.
    pub default: Option<Value>,
    /// Minimum value, when applicable.
    pub min: Option<f64>,
    /// Maximum value, when applicable.
    pub max: Option<f64>,
    /// Slider step, when applicable.
    pub step: Option<f64>,
    /// Enum members, when the prop is an enum.
    pub enum_members: Option<Vec<EnumMember>>,
}

/// Module metadata derived from a parsed MilkDrop preset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilkMeta {
    /// Cleaned display name (title/name without the ` - Author` suffix).
    pub name: String,
    /// Recovered author, or `"unknown"`.
    pub author: String,
    /// Description; currently always empty because the parser discards it.
    pub description: String,
    /// Preset version (1 or 2), mirroring [`MilkDocument::version`].
    pub version: u8,
    /// Always `"effects"`.
    pub category: String,
    /// Always `false` for MilkDrop presets.
    pub preview_with_output: bool,
}

/// Split a preset display name into `(name, author)` using the common
/// `Name - Author` convention.
///
/// The parser folds the preset's `title`/`presetname` line (or, failing that,
/// the file stem) into [`MilkDocument::name`]. When that string contains
/// [`AUTHOR_SEPARATOR`] with a non-empty half on each side, the left half is
/// the display name and the right half is the author. Otherwise the whole
/// trimmed string is the name and [`UNKNOWN_AUTHOR`] is used for the author.
/// Requiring both halves to be non-empty avoids an empty name for an input
/// such as `" - Author"`.
fn split_name_and_author(raw: &str) -> (&str, &str) {
    if let Some((name_part, author_part)) = raw.rsplit_once(AUTHOR_SEPARATOR) {
        let name_part = name_part.trim();
        let author_part = author_part.trim();
        if !name_part.is_empty() && !author_part.is_empty() {
            return (name_part, author_part);
        }
    }
    (raw.trim(), UNKNOWN_AUTHOR)
}

/// Derive module metadata from a parsed preset.
///
/// The author is recovered from the common `Name - Author` convention in
/// [`MilkDocument::name`] via [`split_name_and_author`]; when that convention
/// is absent the author is [`UNKNOWN_AUTHOR`]. The category is always
/// [`CATEGORY`] and `preview_with_output` is always `false` because MilkDrop
/// presets are self-contained effects that never sample the previous frame as
/// a visible input texture.
pub fn meta_from_document(doc: &MilkDocument) -> MilkMeta {
    let (name, author) = split_name_and_author(&doc.name);
    MilkMeta {
        name: name.to_string(),
        author: author.to_string(),
        description: String::new(),
        version: doc.version,
        category: CATEGORY.to_string(),
        preview_with_output: false,
    }
}

/// Names of the preset variables exposed as Inspector input props.
///
/// These are EEL/`MilkBaseVals` names, not UI labels: the compositor copies a
/// module's numeric prop values into [`MilkInstance::overrides`] under these
/// keys, and `begin_frame` applies them after the preset's frame equations so
/// a user control wins over authored animation. Keeping the list here means
/// the prop synthesis and the runtime override path cannot drift apart.
pub const MILKDROP_LOOK_PROP_NAMES: &[&str] = &[
    "decay",
    "gamma",
    "fshader",
    "echo_alpha",
    "echo_zoom",
    "echo_orient",
    "brighten",
    "darken",
    "solarize",
    "invert",
    "red_blue",
    "zoom",
    "warp",
    "rot",
    "wave_a",
    "wave_scale",
    "wave_mode",
    "b1n",
    "b2n",
    "b3n",
];

fn float_prop(
    props: &mut IndexMap<String, MilkProp>,
    name: &str,
    label: &str,
    default: f64,
    min: f64,
    max: f64,
    step: f64,
) {
    props.insert(
        name.to_string(),
        MilkProp {
            type_: MilkPropType::Float,
            label: label.to_string(),
            default: Some(json!(default)),
            min: Some(min),
            max: Some(max),
            step: Some(step),
            enum_members: None,
        },
    );
}

fn bool_prop(props: &mut IndexMap<String, MilkProp>, name: &str, label: &str, default: bool) {
    props.insert(
        name.to_string(),
        MilkProp {
            type_: MilkPropType::Bool,
            label: label.to_string(),
            default: Some(json!(default)),
            min: None,
            max: None,
            step: None,
            enum_members: None,
        },
    );
}

/// Synthesise the module prop table from a parsed preset.
///
/// These are look/dev controls, not a replacement for the preset's EEL: every
/// prop starts at the preset's authored base value and is applied after the
/// per-frame equations, so moving a control visibly overrides the preset while
/// leaving the equation state intact. The curated set covers feedback/decay,
/// the legacy composite (gamma/hue/echo/filters) and the warp/wave basics.
pub fn synthesise_props(doc: &MilkDocument) -> IndexMap<String, MilkProp> {
    let base = &doc.base;
    let mut props = IndexMap::new();

    float_prop(&mut props, "decay", "Decay", base.decay, 0.0, 1.5, 0.001);
    float_prop(&mut props, "gamma", "Gamma", base.gammaadj, 0.0, 8.0, 0.01);
    float_prop(
        &mut props,
        "fshader",
        "Hue Shader",
        base.shader,
        0.0,
        1.0,
        0.01,
    );
    float_prop(
        &mut props,
        "echo_alpha",
        "Echo Alpha",
        base.echo_alpha,
        0.0,
        1.0,
        0.01,
    );
    float_prop(
        &mut props,
        "echo_zoom",
        "Echo Zoom",
        base.echo_zoom,
        0.1,
        4.0,
        0.001,
    );
    let orient = base.echo_orient.clamp(0, 3);
    props.insert(
        "echo_orient".to_string(),
        MilkProp {
            type_: MilkPropType::Enum,
            label: "Echo Orientation".to_string(),
            default: Some(json!(orient)),
            min: None,
            max: None,
            step: None,
            enum_members: Some(
                ["None", "Flip X", "Flip Y", "Flip XY"]
                    .iter()
                    .enumerate()
                    .map(|(index, label)| EnumMember {
                        label: label.to_string(),
                        value: json!(index),
                        selected: index as i64 == orient,
                    })
                    .collect(),
            ),
        },
    );
    bool_prop(&mut props, "brighten", "Brighten", base.brighten);
    bool_prop(&mut props, "darken", "Darken", base.darken);
    bool_prop(&mut props, "solarize", "Solarize", base.solarize);
    bool_prop(&mut props, "invert", "Invert", base.invert);
    bool_prop(&mut props, "red_blue", "Red/Blue Swap", base.red_blue);
    float_prop(&mut props, "zoom", "Zoom", base.zoom, 0.5, 2.0, 0.001);
    float_prop(&mut props, "warp", "Warp", base.warp, 0.0, 4.0, 0.01);
    float_prop(
        &mut props,
        "rot",
        "Rotation",
        base.rot,
        -std::f64::consts::PI,
        std::f64::consts::PI,
        0.001,
    );
    float_prop(
        &mut props,
        "wave_a",
        "Wave Alpha",
        base.wave_a,
        0.0,
        1.0,
        0.01,
    );
    float_prop(
        &mut props,
        "wave_scale",
        "Wave Scale",
        base.wave_scale,
        0.0,
        4.0,
        0.001,
    );
    let wave_mode = base.wave_mode.rem_euclid(8);
    props.insert(
        "wave_mode".to_string(),
        MilkProp {
            type_: MilkPropType::Enum,
            label: "Wave Mode".to_string(),
            default: Some(json!(wave_mode)),
            min: None,
            max: None,
            step: None,
            enum_members: Some(
                [
                    "Circle",
                    "Radial",
                    "Spiral",
                    "Spectrum",
                    "MilkDrop 4",
                    "MilkDrop 5",
                    "Radial 3D",
                    "Dual Line",
                ]
                .iter()
                .enumerate()
                .map(|(index, label)| EnumMember {
                    label: label.to_string(),
                    value: json!(index),
                    selected: index as i64 == wave_mode,
                })
                .collect(),
            ),
        },
    );
    float_prop(&mut props, "b1n", "Blur 1 Min", base.b1n, 0.0, 1.0, 0.001);
    float_prop(&mut props, "b2n", "Blur 2 Min", base.b2n, 0.0, 1.0, 0.001);
    float_prop(&mut props, "b3n", "Blur 3 Min", base.b3n, 0.0, 1.0, 0.001);

    props
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::milkdrop::parse_milk;

    fn parse(src: &str, stem: &str) -> MilkDocument {
        parse_milk(src, stem).expect("test preset parses")
    }

    #[test]
    fn author_from_title_line_with_author_suffix() {
        // A preset `title=` line is folded into `doc.name` by the parser.
        let doc = parse(
            "[preset00]\ntitle=Starfield Drift - Alex Rivas\n",
            "fallback_stem",
        );
        assert_eq!(doc.name, "Starfield Drift - Alex Rivas");
        let meta = meta_from_document(&doc);
        assert_eq!(meta.author, "Alex Rivas");
        assert_eq!(meta.name, "Starfield Drift");
    }

    #[test]
    fn author_from_filename_stem_with_author_suffix() {
        // With no title line, `doc.name` is the file stem handed to parse_milk.
        let doc = parse("[preset00]\n", "Ocean Drive - Jordan Lee");
        assert_eq!(doc.name, "Ocean Drive - Jordan Lee");
        let meta = meta_from_document(&doc);
        assert_eq!(meta.author, "Jordan Lee");
        assert_eq!(meta.name, "Ocean Drive");
    }

    #[test]
    fn plain_name_has_unknown_author() {
        let doc = parse("[preset00]\n", "PlainPreset");
        assert_eq!(doc.name, "PlainPreset");
        let meta = meta_from_document(&doc);
        assert_eq!(meta.author, "unknown");
        assert_eq!(meta.name, "PlainPreset");
    }

    #[test]
    fn synthesise_props_exposes_the_look_controls() {
        let doc = parse(
            "[preset00]\nfDecay=0.91\nfGammaAdj=2.5\nfShader=0.75\n\
             fVideoEchoAlpha=0.4\nfVideoEchoZoom=1.5\nnVideoEchoOrientation=2\n\
             nWaveMode=7\nbDarken=1\nbSolarize=1\nfWaveAlpha=0.3\nfWaveScale=2.0\n",
            "p",
        );
        let props = synthesise_props(&doc);
        assert_eq!(props.len(), MILKDROP_LOOK_PROP_NAMES.len());
        for name in MILKDROP_LOOK_PROP_NAMES {
            assert!(props.contains_key(*name), "missing prop {name}");
        }
        assert_eq!(props["decay"].default, Some(json!(0.91)));
        assert_eq!(props["gamma"].default, Some(json!(2.5)));
        assert_eq!(props["fshader"].default, Some(json!(0.75)));
        assert_eq!(props["echo_alpha"].default, Some(json!(0.4)));
        assert_eq!(props["echo_zoom"].default, Some(json!(1.5)));
        assert_eq!(props["echo_orient"].default, Some(json!(2)));
        assert_eq!(props["wave_mode"].default, Some(json!(7)));
        assert_eq!(props["darken"].default, Some(json!(true)));
        assert_eq!(props["solarize"].default, Some(json!(true)));
        assert_eq!(props["wave_a"].default, Some(json!(0.3)));
        assert_eq!(props["wave_scale"].default, Some(json!(2.0)));
        assert_eq!(props["brighten"].type_, MilkPropType::Bool);
        assert_eq!(props["wave_mode"].type_, MilkPropType::Enum);
        assert_eq!(
            props["wave_mode"]
                .enum_members
                .as_ref()
                .expect("wave mode options")
                .len(),
            8
        );
    }

    #[test]
    fn meta_category_preview_version_name_and_description() {
        let v1 = parse("[preset00]\ntitle=Nebula - Casey Moon\n", "stem");
        let meta = meta_from_document(&v1);
        assert_eq!(meta.category, "effects");
        assert!(!meta.preview_with_output);
        assert_eq!(meta.version, v1.version);
        assert_eq!(meta.version, 1);
        assert_eq!(meta.name, "Nebula");
        assert_eq!(meta.description, "");

        // A v2 preset must mirror the parser's version rather than hard-code 1.
        let v2 = parse(
            "[preset00]\nMILKDROP_PRESET_VERSION=201\ntitle=Warp Field\n",
            "stem",
        );
        let meta = meta_from_document(&v2);
        assert_eq!(meta.version, v2.version);
        assert_eq!(meta.version, 2);
        assert_eq!(meta.author, "unknown");
        assert_eq!(meta.name, "Warp Field");
        assert_eq!(meta.category, "effects");
        assert!(!meta.preview_with_output);
    }
}
