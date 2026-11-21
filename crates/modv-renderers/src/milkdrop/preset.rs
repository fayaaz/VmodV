//! MilkDrop `.mlk` preset parser.
//!
//! The parser is extension-agnostic. It normalises CRLF, finds the
//! `[preset00]` section, splits base-value lines from equation/shader lines,
//! lowercases and compatibility-renames keys, concatenates numbered lines in
//! numeric order, and records unrecognised keys in [`MilkDocument::diagnostics`]
//! instead of dropping them silently.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
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
        self.waves.iter().filter(|w| w.enabled).collect()
    }

    /// Enabled shape sections, in index order.
    pub fn active_shapes(&self) -> Vec<&MilkShape> {
        self.shapes.iter().filter(|s| s.enabled).collect()
    }
}

/// Canonical MilkDrop base values applied before any file overrides.
fn default_base_vals() -> MilkBaseVals {
    MilkBaseVals {
        decay: 1.0,
        gammaadj: 2.0,
        zoom: 1.0,
        zoomexp: 1.0,
        warp: 1.0,
        rot: 0.0,
        cx: 0.5,
        cy: 0.5,
        dx: 0.0,
        dy: 0.0,
        sx: 1.0,
        sy: 1.0,
        echo_zoom: 1.0,
        echo_alpha: 1.0,
        echo_orient: 0,
        wave_mode: 0,
        additivewave: false,
        wave_dots: false,
        wave_thick: false,
        wave_brighten: true,
        wrap: false,
        darken_center: true,
        red_blue: false,
        brighten: false,
        darken: false,
        solarize: false,
        invert: false,
        wave_a: 1.0,
        wave_scale: 1.0,
        wave_smoothing: 0.0,
        wave_mystery: 1.0,
        modwavealphastart: 0.0,
        modwavealphaend: 1.0,
        warpanimspeed: 1.0,
        warpscale: 1.0,
        mv_x: 0,
        mv_y: 0,
        mv_a: 1.0,
        mv_r: 0.0,
        mv_g: 0.0,
        mv_b: 0.0,
        mv_len: 0.0,
        ob_size: 0.02,
        ob_a: 1.0,
        ib_size: 0.02,
        ib_a: 1.0,
        b1n: 0.0,
        b2n: 0.0,
        b3n: 0.0,
        brightness: 1.0,
    }
}

fn parse_f64(value: &str) -> f64 {
    value.trim().parse::<f64>().unwrap_or(0.0)
}

fn parse_i64(value: &str) -> i64 {
    value.trim().parse::<i64>().unwrap_or(0)
}

fn parse_bool(value: &str) -> bool {
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true")
}

/// Apply one `key=value` base val to `base`. `key` is already lowercased;
/// MilkDrop compatibility aliases are folded onto canonical field names here.
/// Unrecognised keys are appended to `diagnostics` rather than dropped.
fn apply_base_val(base: &mut MilkBaseVals, key: &str, value: &str, diagnostics: &mut Vec<String>) {
    match key {
        "decay" | "fdecay" => base.decay = parse_f64(value),
        "gammaadj" | "fgammaadj" => base.gammaadj = parse_f64(value),
        "zoom" | "fzoom" => base.zoom = parse_f64(value),
        "zoomexp" | "fzoomexponent" => base.zoomexp = parse_f64(value),
        "warp" | "fwarp" => base.warp = parse_f64(value),
        "rot" | "frot" => base.rot = parse_f64(value),
        "cx" | "fcx" => base.cx = parse_f64(value),
        "cy" | "fcy" => base.cy = parse_f64(value),
        "dx" | "fdx" => base.dx = parse_f64(value),
        "dy" | "fdy" => base.dy = parse_f64(value),
        "sx" | "fsx" => base.sx = parse_f64(value),
        "sy" | "fsy" => base.sy = parse_f64(value),
        "echo_zoom" | "fvideoechozoom" => base.echo_zoom = parse_f64(value),
        "echo_alpha" | "fvideoechoalpha" => base.echo_alpha = parse_f64(value),
        "echo_orient" | "nvideoechoorientation" => base.echo_orient = parse_i64(value),
        "wave_mode" | "nwavemode" => base.wave_mode = parse_i64(value),
        "additivewave" | "badditivewaves" => base.additivewave = parse_bool(value),
        "wave_dots" | "bwavedots" => base.wave_dots = parse_bool(value),
        "wave_thick" | "bwavethick" => base.wave_thick = parse_bool(value),
        "wave_brighten" | "bmaximizewavecolor" => base.wave_brighten = parse_bool(value),
        "wrap" | "btexwrap" => base.wrap = parse_bool(value),
        "darken_center" | "bdarkencenter" => base.darken_center = parse_bool(value),
        "red_blue" | "bredbluestereo" => base.red_blue = parse_bool(value),
        "brighten" | "bbrighten" => base.brighten = parse_bool(value),
        "darken" | "bdarken" => base.darken = parse_bool(value),
        "solarize" | "bsolarize" => base.solarize = parse_bool(value),
        "invert" | "binvert" => base.invert = parse_bool(value),
        "wave_a" | "fwavealpha" => base.wave_a = parse_f64(value),
        "wave_scale" | "fwavescale" => base.wave_scale = parse_f64(value),
        "wave_smoothing" | "fwavesmoothing" => base.wave_smoothing = parse_f64(value),
        "wave_mystery" | "fwaveparam" => base.wave_mystery = parse_f64(value),
        "modwavealphastart" | "fmodwavealphastart" => base.modwavealphastart = parse_f64(value),
        "modwavealphaend" | "fmodwavealphaend" => base.modwavealphaend = parse_f64(value),
        "warpanimspeed" | "fwarpanimspeed" => base.warpanimspeed = parse_f64(value),
        "warpscale" | "fwarpscale" => base.warpscale = parse_f64(value),
        "mv_x" | "nmotionvectorsx" => base.mv_x = parse_i64(value),
        "mv_y" | "nmotionvectorsy" => base.mv_y = parse_i64(value),
        "mv_a" => base.mv_a = parse_f64(value),
        "mv_r" => base.mv_r = parse_f64(value),
        "mv_g" => base.mv_g = parse_f64(value),
        "mv_b" => base.mv_b = parse_f64(value),
        "mv_len" => base.mv_len = parse_f64(value),
        "ob_size" => base.ob_size = parse_f64(value),
        "ob_a" => base.ob_a = parse_f64(value),
        "ib_size" => base.ib_size = parse_f64(value),
        "ib_a" => base.ib_a = parse_f64(value),
        "b1n" | "fb1n" => base.b1n = parse_f64(value),
        "b2n" | "fb2n" => base.b2n = parse_f64(value),
        "b3n" | "fb3n" => base.b3n = parse_f64(value),
        "brightness" | "fbrightness" => base.brightness = parse_f64(value),
        // Metadata keys are recognised but carry no scalar state.
        "milkdrop_preset_version"
        | "frating"
        | "fshader"
        | "fpreview"
        | "author"
        | "desc"
        | "description" => {}
        _ => diagnostics.push(format!("unknown base val: {key}={value}")),
    }
}

/// Collect numbered `prefix_N` lines and join their values in numeric order.
#[derive(Default)]
struct EqLines {
    lines: BTreeMap<usize, String>,
}

impl EqLines {
    fn push(&mut self, index: usize, value: &str) {
        self.lines.insert(index, value.to_string());
    }

    fn into_source(self) -> String {
        self.lines.into_values().collect::<Vec<_>>().join("\n")
    }
}

/// Accumulator for one wave or shape section.
#[derive(Default)]
struct SectionAcc {
    enabled: bool,
    base: MilkBaseVals,
    init: EqLines,
    frame: EqLines,
    point: EqLines,
}

impl SectionAcc {
    fn new() -> Self {
        Self {
            base: default_base_vals(),
            ..Default::default()
        }
    }
}

/// If `key` is `prefix` or `prefix_N`, return its numeric index (bare → 0).
fn numbered_index(key: &str, prefix: &str) -> Option<usize> {
    if key == prefix {
        return Some(0);
    }
    key.strip_prefix(prefix)?
        .strip_prefix('_')?
        .parse::<usize>()
        .ok()
}

/// Parse `wavecode_<i>_<prop>` / `shapecode_<i>_<prop>`; `i` is 1-based.
fn parse_section_prop(rest: &str) -> Option<(usize, &str)> {
    let (num, prop) = rest.split_once('_')?;
    let i = num.parse::<usize>().ok()?;
    if i == 0 || i > 4 {
        return None;
    }
    Some((i - 1, prop))
}

/// Parse `wave_<i>_<kind>[_N]` / `shape_<i>_<kind>[_N]`.
fn parse_section_eq(rest: &str) -> Option<(usize, &str, usize)> {
    let (num, tail) = rest.split_once('_')?;
    let i = num.parse::<usize>().ok()?;
    if i == 0 || i > 4 {
        return None;
    }
    let (kind, index) = match tail.rsplit_once('_') {
        Some((kind, n)) => (kind, n.parse::<usize>().ok()?),
        None => (tail, 0),
    };
    Some((i - 1, kind, index))
}

/// Drop noise-sampler redeclaration lines, keeping their use sites.
fn strip_noise_decls(text: &str) -> String {
    text.split('\n')
        .filter(|line| {
            let lower = line.to_lowercase();
            !(lower.contains("sampler2d")
                && (lower.contains("sampler_pw_noise_lq") || lower.contains("sampler_pw_noise_hq")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Return the text between the first `{` after `from` and its matching `}`.
fn brace_interior(from: &str) -> Option<&str> {
    let open = from.find('{')?;
    let mut depth = 0usize;
    for (offset, ch) in from[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&from[open + 1..open + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split assembled shader text at `shader_body`, returning the declaration
/// prelude and the braces' interior, with `sampler_pw_noise_lq/hq`
/// redeclarations removed.
pub fn extract_shader_body(src: &str) -> MilkShader {
    let Some(pos) = src.to_lowercase().find("shader_body") else {
        return MilkShader {
            prelude: strip_noise_decls(src),
            body: String::new(),
        };
    };
    let prelude = strip_noise_decls(&src[..pos]);
    let after = &src[pos + "shader_body".len()..];
    let body = strip_noise_decls(brace_interior(after).unwrap_or(after));
    MilkShader { prelude, body }
}

/// Parse a MilkDrop preset from `src`. The file extension is irrelevant.
pub fn parse_milk(src: &str, file_stem: &str) -> Result<MilkDocument, MilkParseError> {
    let normalized = src.replace("\r\n", "\n");

    let section_start = normalized
        .lines()
        .position(|line| line.trim().eq_ignore_ascii_case("[preset00]"))
        .ok_or(MilkParseError::PresetSectionMissing)?;

    let version = if normalized
        .to_lowercase()
        .contains("milkdrop_preset_version=201")
    {
        2
    } else {
        1
    };

    let mut doc = MilkDocument {
        version,
        name: file_stem.to_string(),
        base: default_base_vals(),
        waves: std::array::from_fn(|_| MilkWave::default()),
        shapes: std::array::from_fn(|_| MilkShape::default()),
        ..Default::default()
    };

    let mut init_eqs = EqLines::default();
    let mut frame_eqs = EqLines::default();
    let mut pixel_eqs = EqLines::default();
    let mut warp_lines = EqLines::default();
    let mut comp_lines = EqLines::default();
    let mut wave_acc: [SectionAcc; 4] = std::array::from_fn(|_| SectionAcc::new());
    let mut shape_acc: [SectionAcc; 4] = std::array::from_fn(|_| SectionAcc::new());

    for line in normalized.lines().skip(section_start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(';')
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with('[')
        {
            continue;
        }
        let Some((raw_key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().to_lowercase();
        // `value` deliberately keeps leading whitespace: EEL/shader lines rely
        // on indentation; scalar parsers trim locally.

        if key == "title" || key == "presetname" {
            if !value.trim().is_empty() {
                doc.name = value.trim().to_string();
            }
            continue;
        }

        if let Some(index) = numbered_index(&key, "per_frame_init") {
            init_eqs.push(index, value);
            continue;
        }
        if let Some(index) = numbered_index(&key, "per_frame") {
            frame_eqs.push(index, value);
            continue;
        }
        if let Some(index) = numbered_index(&key, "per_pixel") {
            pixel_eqs.push(index, value);
            continue;
        }
        if let Some(index) = numbered_index(&key, "warp") {
            warp_lines.push(index, value);
            continue;
        }
        if let Some(index) = numbered_index(&key, "comp") {
            comp_lines.push(index, value);
            continue;
        }

        if let Some(rest) = key.strip_prefix("wavecode_")
            && let Some((i, prop)) = parse_section_prop(rest)
        {
            let section = &mut wave_acc[i];
            if prop == "enabled" {
                section.enabled = parse_bool(value);
            } else {
                apply_base_val(&mut section.base, prop, value, &mut doc.diagnostics);
            }
            continue;
        }
        if let Some(rest) = key.strip_prefix("shapecode_")
            && let Some((i, prop)) = parse_section_prop(rest)
        {
            let section = &mut shape_acc[i];
            if prop == "enabled" {
                section.enabled = parse_bool(value);
            } else {
                apply_base_val(&mut section.base, prop, value, &mut doc.diagnostics);
            }
            continue;
        }
        if let Some(rest) = key.strip_prefix("wave_")
            && let Some((i, kind, index)) = parse_section_eq(rest)
        {
            let section = &mut wave_acc[i];
            match kind {
                "init" => section.init.push(index, value),
                "per_frame" => section.frame.push(index, value),
                "per_point" => section.point.push(index, value),
                _ => {}
            }
            continue;
        }
        if let Some(rest) = key.strip_prefix("shape_")
            && let Some((i, kind, index)) = parse_section_eq(rest)
        {
            let section = &mut shape_acc[i];
            match kind {
                "init" => section.init.push(index, value),
                "per_frame" => section.frame.push(index, value),
                _ => {}
            }
            continue;
        }

        apply_base_val(&mut doc.base, &key, value, &mut doc.diagnostics);
    }

    doc.init_eqs = init_eqs.into_source();
    doc.frame_eqs = frame_eqs.into_source();
    doc.pixel_eqs = pixel_eqs.into_source();
    doc.warp = strip_noise_decls(&warp_lines.into_source());
    doc.comp = strip_noise_decls(&comp_lines.into_source());

    for (i, section) in wave_acc.into_iter().enumerate() {
        doc.waves[i] = MilkWave {
            enabled: section.enabled,
            base: section.base,
            init_eqs: section.init.into_source(),
            frame_eqs: section.frame.into_source(),
            point_eqs: section.point.into_source(),
        };
    }
    for (i, section) in shape_acc.into_iter().enumerate() {
        doc.shapes[i] = MilkShape {
            enabled: section.enabled,
            base: section.base,
            init_eqs: section.init.into_source(),
            frame_eqs: section.frame.into_source(),
        };
    }

    Ok(doc)
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
        assert!(
            (doc.base.decay - 1.0).abs() < 1e-9,
            "defaults still apply in v2"
        );
    }

    #[test]
    fn missing_preset_section_is_a_typed_error() {
        let result = parse_milk("fDecay=0.9\nper_frame_1=x=1;\n", "no_section");
        assert!(
            result.is_err(),
            "a file without [preset00] must be rejected"
        );
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
            doc.diagnostics
                .iter()
                .any(|d| d.to_lowercase().contains("unknownfuturekey")),
            "unknown key must be recorded, got {:?}",
            doc.diagnostics
        );
    }
}
