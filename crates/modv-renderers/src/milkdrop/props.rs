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
use serde_json::Value;

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

/// Synthesise the module prop table from a parsed preset.
///
/// MilkDrop presets currently expose no user-facing props: their parameters
/// are driven by EEL equations evaluated each frame rather than by editable
/// inputs, so the returned ordered map is intentionally empty. The function
/// exists so the module registry can share one `document -> props` conversion
/// between ISF and MilkDrop modules; later layers may populate per-preset
/// props without changing any caller.
pub fn synthesise_props(_doc: &MilkDocument) -> IndexMap<String, MilkProp> {
    IndexMap::new()
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
    fn synthesise_props_returns_empty_index_map() {
        let doc = parse("[preset00]\nper_frame_1=q=1;\n", "p");
        let props = synthesise_props(&doc);
        assert!(props.is_empty());
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
