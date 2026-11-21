//! MilkDrop `.mlk` preset parser.
//!
//! RED-phase scaffolding: the public API below is fixed by the tests in
//! [`tests`]; GREEN replaces the stub bodies with the real parser.
//!
//! The parser is extension-agnostic. It normalises CRLF, finds the
//! `[preset00]` section, splits base-value lines from equation/shader lines,
//! lowercases and compatibility-renames keys, concatenates numbered lines in
//! numeric order, and records unrecognised keys in [`MilkDocument::diagnostics`]
//! instead of dropping them silently.

#![forbid(unsafe_code)]

use std::fmt;

/// Errors from [`parse_milk`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MilkParseError {
    /// The file has no `[preset00]` section (case-insensitive).
    PresetSectionMissing,
}

impl fmt::Display for MilkParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MilkParseError::PresetSectionMissing => {
                write!(f, "MilkDrop preset is missing the [preset00] section")
            }
        }
    }
}

impl std::error::Error for MilkParseError {}

/// The source text of one group of EEL equations, compiled later by the EEL
/// layer. Kept as a distinct type so the compile step can cache handles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MilkEquationSet {
    /// Raw, newline-joined EEL source.
    pub source: String,
}

/// A warp/composite shader split at the `shader_body` sentinel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MilkShader {
    /// Declarations that appeared before `shader_body`, with noise-sampler
    /// redeclarations removed.
    pub prelude: String,
    /// The interior of the `shader_body { … }` braces.
    pub body: String,
}

/// Scalar preset state: the `key=value` lines outside equation/shader blocks.
///
/// Defaults are applied at parse time; only keys that appeared in the file
/// override them. Unknown keys are recorded on the document, not here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilkBaseVals {
    pub decay: f64,
    pub gammaadj: f64,
    pub zoom: f64,
    pub zoomexp: f64,
    pub warp: f64,
    pub rot: f64,
    pub cx: f64,
    pub cy: f64,
    pub dx: f64,
    pub dy: f64,
    pub sx: f64,
    pub sy: f64,
    pub echo_zoom: f64,
    pub echo_alpha: f64,
    pub echo_orient: i64,
    pub wave_mode: i64,
    pub additivewave: bool,
    pub wave_dots: bool,
    pub wave_thick: bool,
    pub wave_brighten: bool,
    pub wrap: bool,
    pub darken_center: bool,
    pub red_blue: bool,
    pub brighten: bool,
    pub darken: bool,
    pub solarize: bool,
    pub invert: bool,
    pub wave_a: f64,
    pub wave_scale: f64,
    pub wave_smoothing: f64,
    pub wave_mystery: f64,
    pub modwavealphastart: f64,
    pub modwavealphaend: f64,
    pub warpanimspeed: f64,
    pub warpscale: f64,
    pub mv_x: i64,
    pub mv_y: i64,
    pub mv_a: f64,
    pub mv_r: f64,
    pub mv_g: f64,
    pub mv_b: f64,
    pub mv_len: f64,
    pub ob_size: f64,
    pub ob_a: f64,
    pub ib_size: f64,
    pub ib_a: f64,
    pub b1n: f64,
    pub b2n: f64,
    pub b3n: f64,
    pub brightness: f64,
}

/// One of the four wave sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilkWave {
    /// From `wavecode_<i>_enabled`; false means the section is ignored.
    pub enabled: bool,
    pub base: MilkBaseVals,
    pub init_eqs: String,
    pub frame_eqs: String,
    pub point_eqs: String,
}

/// One of the four shape sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilkShape {
    /// From `shapecode_<i>_enabled`; false means the section is ignored.
    pub enabled: bool,
    pub base: MilkBaseVals,
    pub init_eqs: String,
    pub frame_eqs: String,
}

/// A parsed MilkDrop preset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilkDocument {
    /// 1 or 2; 2 when `MILKDROP_PRESET_VERSION=201` is present.
    pub version: u8,
    /// File stem, unless the preset carries a title line.
    pub name: String,
    pub base: MilkBaseVals,
    pub init_eqs: String,
    pub frame_eqs: String,
    pub pixel_eqs: String,
    pub waves: [MilkWave; 4],
    pub shapes: [MilkShape; 4],
    /// Assembled warp shader source (noise redeclarations stripped).
    pub warp: String,
    /// Assembled composite shader source (noise redeclarations stripped).
    pub comp: String,
    /// Unrecognised base-val keys, never silently discarded.
    pub diagnostics: Vec<String>,
}

impl MilkDocument {
    /// Enabled wave sections, in index order.
    pub fn active_waves(&self) -> Vec<&MilkWave> {
        // STUB (RED): the real implementation filters by `enabled`.
        Vec::new()
    }

    /// Enabled shape sections, in index order.
    pub fn active_shapes(&self) -> Vec<&MilkShape> {
        // STUB (RED): the real implementation filters by `enabled`.
        Vec::new()
    }
}

/// Split assembled shader text at `shader_body`, returning the declaration
/// prelude and the braces' interior, with `sampler_pw_noise_lq/hq`
/// redeclarations removed.
pub fn extract_shader_body(_src: &str) -> MilkShader {
    // STUB (RED).
    MilkShader::default()
}

/// Parse a MilkDrop preset from `src`. The file extension is irrelevant.
pub fn parse_milk(_src: &str, _file_stem: &str) -> Result<MilkDocument, MilkParseError> {
    // STUB (RED) — enough for the tests to compile and fail on assertions.
    Ok(MilkDocument::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "\
[preset00]
fRating=5.0
per_frame_1=wave_x = 0.5;
";

    const V2: &str = "\
[preset00]
MILKDROP_PRESET_VERSION=201
fRating=3.0
per_frame_1=wave_y = 0.5;
";

    #[test]
    fn minimal_v1_fixture_parses_with_name_and_base_defaults() {
        let doc = parse_milk(V1, "v1_preset").expect("v1 preset parses");
        assert_eq!(doc.version, 1, "no version marker means v1");
        assert_eq!(doc.name, "v1_preset", "name comes from the file stem");
        assert!((doc.base.decay - 1.0).abs() < 1e-9, "default decay");
        assert!((doc.base.zoom - 1.0).abs() < 1e-9, "default zoom");
        assert!((doc.base.cx - 0.5).abs() < 1e-9, "default cx");
        assert!((doc.base.cy - 0.5).abs() < 1e-9, "default cy");
    }

    #[test]
    fn minimal_v2_fixture_parses_version_and_name() {
        let doc = parse_milk(V2, "v2_preset").expect("v2 preset parses");
        assert_eq!(doc.version, 2, "MILKDROP_PRESET_VERSION=201 means v2");
        assert_eq!(doc.name, "v2_preset");
        assert!((doc.base.decay - 1.0).abs() < 1e-9, "defaults still apply in v2");
    }

    #[test]
    fn missing_preset_section_is_a_typed_error() {
        let result = parse_milk("fDecay=0.9\nper_frame_1=x=1;\n", "no_section");
        assert!(result.is_err(), "a file without [preset00] must be rejected");
        assert!(matches!(
            result.err(),
            Some(MilkParseError::PresetSectionMissing)
        ));
    }

    #[test]
    fn crlf_and_out_of_order_numbered_lines_join_in_numeric_order() {
        let src = "[preset00]\r\n\
                   per_frame_2=frame_b = 2.0;\r\n\
                   per_frame_1=frame_a = 1.0;\r\n\
                   warp_4=}\r\n\
                   warp_1=shader_body {\r\n\
                   warp_3=  b = 2.0;\r\n\
                   warp_2=  a = 1.0;\r\n";
        let doc = parse_milk(src, "ordered").expect("parses");
        assert_eq!(
            doc.frame_eqs, "frame_a = 1.0;\nframe_b = 2.0;",
            "per_frame_N must be concatenated in numeric order with LF"
        );
        assert!(
            doc.warp.contains("  a = 1.0;\n  b = 2.0;"),
            "warp_N must be concatenated in numeric order, got {:?}",
            doc.warp
        );
        assert!(
            !doc.warp.contains('\r'),
            "CRLF must be normalised to LF, got {:?}",
            doc.warp
        );
    }

    #[test]
    fn compatibility_rename_fdecay_maps_to_decay() {
        let src = "[preset00]\nfDecay=0.98\nper_frame_1=x=1;\n";
        let doc = parse_milk(src, "renamed").expect("parses");
        assert!(
            (doc.base.decay - 0.98).abs() < 1e-9,
            "fDecay must be renamed to decay, got {}",
            doc.base.decay
        );
    }

    #[test]
    fn active_waves_and_shapes_keep_only_enabled_sections() {
        let src = "\
[preset00]
wavecode_1_enabled=1
wavecode_2_enabled=0
shapecode_1_enabled=1
shapecode_2_enabled=0
wave_1_per_frame_1=wave_a = 1.0;
wave_2_per_frame_1=wave_a = 2.0;
shape_1_per_frame_1=shape_a = 1.0;
shape_2_per_frame_1=shape_a = 2.0;
";
        let doc = parse_milk(src, "sections").expect("parses");
        assert_eq!(doc.active_waves().len(), 1, "one enabled wave");
        assert_eq!(doc.active_shapes().len(), 1, "one enabled shape");
        assert!(doc.waves[0].enabled, "wave 1 enabled");
        assert!(!doc.waves[1].enabled, "wave 2 disabled");
        assert!(doc.shapes[0].enabled, "shape 1 enabled");
        assert!(!doc.shapes[1].enabled, "shape 2 disabled");
        assert!(
            doc.active_waves()[0].frame_eqs.contains("1.0"),
            "active wave must be wave 1"
        );
    }

    #[test]
    fn shader_body_is_split_into_prelude_and_body_with_noise_stripped() {
        let src = "\
uniform float time;\n\
uniform sampler2D sampler_pw_noise_lq;\n\
uniform sampler2D sampler_pw_noise_hq;\n\
shader_body\n\
{\n\
  vec4 ret = tex2d(sampler_pw_noise_lq, uv);\n\
}\n";
        let shader = extract_shader_body(src);
        assert_eq!(shader.prelude, "uniform float time;\n");
        assert!(shader.body.contains("vec4 ret"), "body interior kept");
        assert!(
            !shader.prelude.contains("sampler_pw_noise_lq"),
            "noise redeclarations stripped from prelude"
        );
        assert!(
            !shader.prelude.contains("sampler_pw_noise_hq"),
            "noise redeclarations stripped from prelude"
        );
        assert!(
            !shader.body.contains("sampler2D sampler_pw_noise"),
            "noise sampler declarations stripped from body"
        );
    }

    #[test]
    fn unknown_base_val_is_recorded_in_diagnostics() {
        let src = "[preset00]\nfDecay=0.9\nfUnknownFutureKey=42\nper_frame_1=x=1;\n";
        let doc = parse_milk(src, "unknown_key").expect("parses");
        assert!(
            doc.diagnostics.iter().any(|d| d.to_lowercase().contains("unknownfuturekey")),
            "unknown key must be recorded, got {:?}",
            doc.diagnostics
        );
    }
}
