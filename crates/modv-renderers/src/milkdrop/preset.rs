//! MilkDrop `.mlk` preset parser.
//!
//! The parser is extension-agnostic. It normalises CRLF, finds the
//! `[preset00]` section, splits base-value lines from equation/shader lines,
//! lowercases and compatibility-renames keys, concatenates numbered lines in
//! numeric order, and records unrecognised keys in [`MilkDocument::diagnostics`]
//! instead of dropping them silently.
//!
//! Real MilkDrop 2 `.milk` files add a few conventions this parser accepts:
//! they may start with a UTF-8 BOM, every `key=value` line escapes its value
//! with exactly one leading backtick, and they carry `psversion*` metadata
//! keys that are recognised but hold no scalar state (like the rating,
//! shader and preview keys). Only the first backtick is an escape: any
//! indentation, mid-line backticks or further leading backticks belong to the
//! EEL/shader source and are preserved verbatim.
//!
//! [`MilkDocument::warp`] and [`MilkDocument::comp`] keep the assembled source
//! (with the `shader_body` sentinel and braces intact, noise-sampler
//! redeclarations removed). [`extract_shader_body`] is the canonical way for
//! later layers to split that source into a prelude and a body, so brace
//! matching and noise stripping are implemented in exactly one place.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

/// Number of wave/shape slots a MilkDrop preset can define.
const SECTION_COUNT: usize = 4;

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

/// A key family mapping onto one scalar field of [`MilkBaseVals`].
///
/// `keys[0]` is the canonical MilkDrop name; the remaining entries are the
/// compatibility spellings that appear in real `.mlk` files (`f`/`b`/`n`
/// prefixes and legacy names). Keeping the aliases beside the setter makes
/// the rename map impossible to drift from the field it feeds.
struct BaseVal {
    keys: &'static [&'static str],
    set: fn(&mut MilkBaseVals, &str),
}

/// Canonical key → setter table. A line is matched against each family once;
/// ordering is therefore irrelevant (unlike a long `match` with fallthrough).
static BASE_VALS: &[BaseVal] = &[
    BaseVal {
        keys: &["decay", "fdecay"],
        set: |b, v| b.decay = parse_f64(v),
    },
    BaseVal {
        keys: &["gammaadj", "fgammaadj"],
        set: |b, v| b.gammaadj = parse_f64(v),
    },
    BaseVal {
        keys: &["zoom", "fzoom"],
        set: |b, v| b.zoom = parse_f64(v),
    },
    BaseVal {
        keys: &["zoomexp", "fzoomexponent"],
        set: |b, v| b.zoomexp = parse_f64(v),
    },
    BaseVal {
        keys: &["warp", "fwarp"],
        set: |b, v| b.warp = parse_f64(v),
    },
    BaseVal {
        keys: &["rot", "frot"],
        set: |b, v| b.rot = parse_f64(v),
    },
    BaseVal {
        keys: &["cx", "fcx"],
        set: |b, v| b.cx = parse_f64(v),
    },
    BaseVal {
        keys: &["cy", "fcy"],
        set: |b, v| b.cy = parse_f64(v),
    },
    BaseVal {
        keys: &["dx", "fdx"],
        set: |b, v| b.dx = parse_f64(v),
    },
    BaseVal {
        keys: &["dy", "fdy"],
        set: |b, v| b.dy = parse_f64(v),
    },
    BaseVal {
        keys: &["sx", "fsx"],
        set: |b, v| b.sx = parse_f64(v),
    },
    BaseVal {
        keys: &["sy", "fsy"],
        set: |b, v| b.sy = parse_f64(v),
    },
    BaseVal {
        keys: &["echo_zoom", "fvideoechozoom"],
        set: |b, v| b.echo_zoom = parse_f64(v),
    },
    BaseVal {
        keys: &["echo_alpha", "fvideoechoalpha"],
        set: |b, v| b.echo_alpha = parse_f64(v),
    },
    BaseVal {
        keys: &["echo_orient", "nvideoechoorientation"],
        set: |b, v| b.echo_orient = parse_i64(v),
    },
    BaseVal {
        keys: &["wave_mode", "nwavemode"],
        set: |b, v| b.wave_mode = parse_i64(v),
    },
    BaseVal {
        keys: &["additivewave", "badditivewaves"],
        set: |b, v| b.additivewave = parse_bool(v),
    },
    BaseVal {
        keys: &["wave_dots", "bwavedots"],
        set: |b, v| b.wave_dots = parse_bool(v),
    },
    BaseVal {
        keys: &["wave_thick", "bwavethick"],
        set: |b, v| b.wave_thick = parse_bool(v),
    },
    BaseVal {
        keys: &["wave_brighten", "bmaximizewavecolor"],
        set: |b, v| b.wave_brighten = parse_bool(v),
    },
    BaseVal {
        keys: &["wrap", "btexwrap"],
        set: |b, v| b.wrap = parse_bool(v),
    },
    BaseVal {
        keys: &["darken_center", "bdarkencenter"],
        set: |b, v| b.darken_center = parse_bool(v),
    },
    BaseVal {
        keys: &["red_blue", "bredbluestereo"],
        set: |b, v| b.red_blue = parse_bool(v),
    },
    BaseVal {
        keys: &["brighten", "bbrighten"],
        set: |b, v| b.brighten = parse_bool(v),
    },
    BaseVal {
        keys: &["darken", "bdarken"],
        set: |b, v| b.darken = parse_bool(v),
    },
    BaseVal {
        keys: &["solarize", "bsolarize"],
        set: |b, v| b.solarize = parse_bool(v),
    },
    BaseVal {
        keys: &["invert", "binvert"],
        set: |b, v| b.invert = parse_bool(v),
    },
    BaseVal {
        keys: &["wave_a", "fwavealpha"],
        set: |b, v| b.wave_a = parse_f64(v),
    },
    BaseVal {
        keys: &["wave_scale", "fwavescale"],
        set: |b, v| b.wave_scale = parse_f64(v),
    },
    BaseVal {
        keys: &["wave_smoothing", "fwavesmoothing"],
        set: |b, v| b.wave_smoothing = parse_f64(v),
    },
    BaseVal {
        keys: &["wave_mystery", "fwaveparam"],
        set: |b, v| b.wave_mystery = parse_f64(v),
    },
    BaseVal {
        keys: &["modwavealphastart", "fmodwavealphastart"],
        set: |b, v| b.modwavealphastart = parse_f64(v),
    },
    BaseVal {
        keys: &["modwavealphaend", "fmodwavealphaend"],
        set: |b, v| b.modwavealphaend = parse_f64(v),
    },
    BaseVal {
        keys: &["warpanimspeed", "fwarpanimspeed"],
        set: |b, v| b.warpanimspeed = parse_f64(v),
    },
    BaseVal {
        keys: &["warpscale", "fwarpscale"],
        set: |b, v| b.warpscale = parse_f64(v),
    },
    BaseVal {
        keys: &["mv_x", "nmotionvectorsx"],
        set: |b, v| b.mv_x = parse_i64(v),
    },
    BaseVal {
        keys: &["mv_y", "nmotionvectorsy"],
        set: |b, v| b.mv_y = parse_i64(v),
    },
    BaseVal {
        keys: &["mv_a"],
        set: |b, v| b.mv_a = parse_f64(v),
    },
    BaseVal {
        keys: &["mv_r"],
        set: |b, v| b.mv_r = parse_f64(v),
    },
    BaseVal {
        keys: &["mv_g"],
        set: |b, v| b.mv_g = parse_f64(v),
    },
    BaseVal {
        keys: &["mv_b"],
        set: |b, v| b.mv_b = parse_f64(v),
    },
    BaseVal {
        keys: &["mv_len"],
        set: |b, v| b.mv_len = parse_f64(v),
    },
    BaseVal {
        keys: &["ob_size"],
        set: |b, v| b.ob_size = parse_f64(v),
    },
    BaseVal {
        keys: &["ob_a"],
        set: |b, v| b.ob_a = parse_f64(v),
    },
    BaseVal {
        keys: &["ib_size"],
        set: |b, v| b.ib_size = parse_f64(v),
    },
    BaseVal {
        keys: &["ib_a"],
        set: |b, v| b.ib_a = parse_f64(v),
    },
    BaseVal {
        keys: &["b1n", "fb1n"],
        set: |b, v| b.b1n = parse_f64(v),
    },
    BaseVal {
        keys: &["b2n", "fb2n"],
        set: |b, v| b.b2n = parse_f64(v),
    },
    BaseVal {
        keys: &["b3n", "fb3n"],
        set: |b, v| b.b3n = parse_f64(v),
    },
    BaseVal {
        keys: &["brightness", "fbrightness"],
        set: |b, v| b.brightness = parse_f64(v),
    },
];

/// Metadata keys that are valid in a preset but carry no scalar state.
///
/// `milkdrop_preset_version` and the `fRating`/`fShader`/`fPreview` fields are
/// legacy v1 metadata; the `psversion*` trio is the v2 equivalent (shader
/// model versions for the main, warp and composite paths). All are accepted
/// without contributing to [`MilkBaseVals`] or to diagnostics.
static METADATA_KEYS: &[&str] = &[
    "milkdrop_preset_version",
    "frating",
    "fshader",
    "fpreview",
    "psversion",
    "psversion_warp",
    "psversion_comp",
    "author",
    "desc",
    "description",
];

/// Apply one `key=value` base val to `base`. `key` is already lowercased;
/// compatibility aliases are folded onto canonical fields via [`BASE_VALS`].
/// Unrecognised keys are appended to `diagnostics` rather than dropped.
fn apply_base_val(base: &mut MilkBaseVals, key: &str, value: &str, diagnostics: &mut Vec<String>) {
    if let Some(entry) = BASE_VALS.iter().find(|entry| entry.keys.contains(&key)) {
        (entry.set)(base, value);
    } else if !METADATA_KEYS.contains(&key) {
        diagnostics.push(format!("unknown base val: {key}={value}"));
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

    /// Apply a `wavecode_<i>_*` / `shapecode_<i>_*` property. `enabled` is the
    /// only non-scalar property; every other property is a base val.
    fn set_prop(&mut self, prop: &str, value: &str, diagnostics: &mut Vec<String>) {
        if prop == "enabled" {
            self.enabled = parse_bool(value);
        } else {
            apply_base_val(&mut self.base, prop, value, diagnostics);
        }
    }

    /// Route a `wave_<i>_*` / `shape_<i>_*` equation into its accumulator.
    /// Unknown kinds are ignored (shapes have no `per_point`, for instance).
    fn push_eq(&mut self, kind: &str, index: usize, value: &str) {
        match kind {
            "init" => self.init.push(index, value),
            "per_frame" => self.frame.push(index, value),
            "per_point" => self.point.push(index, value),
            _ => {}
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

/// Route a numbered line into the first matching equation set.
fn push_numbered(key: &str, value: &str, sets: &mut [(&str, &mut EqLines)]) -> bool {
    for (prefix, lines) in sets.iter_mut() {
        if let Some(index) = numbered_index(key, prefix) {
            lines.push(index, value);
            return true;
        }
    }
    false
}

/// Convert a 1-based section number into a 0-based slot index.
fn section_index(num: &str) -> Option<usize> {
    let i = num.parse::<usize>().ok()?;
    (i > 0 && i <= SECTION_COUNT).then_some(i - 1)
}

/// Parse `wavecode_<i>_<prop>` / `shapecode_<i>_<prop>`; `i` is 1-based.
fn parse_section_prop(rest: &str) -> Option<(usize, &str)> {
    let (num, prop) = rest.split_once('_')?;
    Some((section_index(num)?, prop))
}

/// Parse `wave_<i>_<kind>[_N]` / `shape_<i>_<kind>[_N]`.
fn parse_section_eq(rest: &str) -> Option<(usize, &str, usize)> {
    let (num, tail) = rest.split_once('_')?;
    let i = section_index(num)?;
    let (kind, index) = match tail.rsplit_once('_') {
        Some((kind, n)) => (kind, n.parse::<usize>().ok()?),
        None => (tail, 0),
    };
    Some((i, kind, index))
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

/// Remove the single leading backtick a real MilkDrop 2 preset uses to escape
/// each `key=value` line. The backtick is a file-format escape, never part of
/// the EEL/shader source: only one is removed, so indentation, mid-line
/// backticks and any further leading backticks are preserved verbatim.
fn unescape_preset_value(raw_value: &str) -> &str {
    raw_value.strip_prefix('`').unwrap_or(raw_value)
}

/// Parse a MilkDrop preset from `src`. The file extension is irrelevant.
pub fn parse_milk(src: &str, file_stem: &str) -> Result<MilkDocument, MilkParseError> {
    let owned = src.replace("\r\n", "\n");
    // Real `.milk` files frequently begin with a UTF-8 BOM. Strip exactly one
    // before section detection; a BOM alone does not create a section.
    let normalized = owned.strip_prefix('\u{feff}').unwrap_or(&owned);

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

    let mut name: Option<String> = None;
    let mut base = default_base_vals();
    let mut diagnostics = Vec::new();
    let mut init_eqs = EqLines::default();
    let mut frame_eqs = EqLines::default();
    let mut pixel_eqs = EqLines::default();
    let mut warp_lines = EqLines::default();
    let mut comp_lines = EqLines::default();
    let mut wave_acc: [SectionAcc; SECTION_COUNT] = std::array::from_fn(|_| SectionAcc::new());
    let mut shape_acc: [SectionAcc; SECTION_COUNT] = std::array::from_fn(|_| SectionAcc::new());

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
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().to_lowercase();
        let value = unescape_preset_value(raw_value);
        // `value` deliberately keeps leading whitespace: EEL/shader lines rely
        // on indentation; scalar parsers trim locally.

        if key == "title" || key == "presetname" {
            if !value.trim().is_empty() {
                name = Some(value.trim().to_string());
            }
            continue;
        }

        if push_numbered(
            &key,
            value,
            &mut [
                ("per_frame_init", &mut init_eqs),
                ("per_frame", &mut frame_eqs),
                ("per_pixel", &mut pixel_eqs),
                ("warp", &mut warp_lines),
                ("comp", &mut comp_lines),
            ],
        ) {
            continue;
        }

        if let Some(rest) = key.strip_prefix("wavecode_")
            && let Some((i, prop)) = parse_section_prop(rest)
        {
            wave_acc[i].set_prop(prop, value, &mut diagnostics);
            continue;
        }
        if let Some(rest) = key.strip_prefix("shapecode_")
            && let Some((i, prop)) = parse_section_prop(rest)
        {
            shape_acc[i].set_prop(prop, value, &mut diagnostics);
            continue;
        }
        if let Some(rest) = key.strip_prefix("wave_")
            && let Some((i, kind, index)) = parse_section_eq(rest)
        {
            wave_acc[i].push_eq(kind, index, value);
            continue;
        }
        if let Some(rest) = key.strip_prefix("shape_")
            && let Some((i, kind, index)) = parse_section_eq(rest)
        {
            shape_acc[i].push_eq(kind, index, value);
            continue;
        }

        apply_base_val(&mut base, &key, value, &mut diagnostics);
    }

    let waves = wave_acc.map(|section| MilkWave {
        enabled: section.enabled,
        base: section.base,
        init_eqs: section.init.into_source(),
        frame_eqs: section.frame.into_source(),
        point_eqs: section.point.into_source(),
    });
    let shapes = shape_acc.map(|section| MilkShape {
        enabled: section.enabled,
        base: section.base,
        init_eqs: section.init.into_source(),
        frame_eqs: section.frame.into_source(),
    });

    Ok(MilkDocument {
        version,
        name: name.unwrap_or_else(|| file_stem.to_string()),
        base,
        init_eqs: init_eqs.into_source(),
        frame_eqs: frame_eqs.into_source(),
        pixel_eqs: pixel_eqs.into_source(),
        waves,
        shapes,
        warp: strip_noise_decls(&warp_lines.into_source()),
        comp: strip_noise_decls(&comp_lines.into_source()),
        diagnostics,
    })
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

    // ---- Real MilkDrop 2 `.milk` format compatibility (RED) -----------------

    /// Real v2 presets escape every raw shader line by prefixing it with a
    /// backtick. That backtick is a file-format escape, never GLSL/HLSL.
    #[test]
    fn backtick_escaped_shader_lines_are_stripped_and_body_extracts() {
        let src = r#"[preset00]
MILKDROP_PRESET_VERSION=201
warp_1=`float4 helper(float2 p) {
warp_2=`  return float4(p, 0.0, 1.0);
warp_3=`}
warp_4=`shader_body
warp_5=`{
warp_6=`  float3 ret = helper(uv_orig).rgb;
warp_7=`}
comp_1=`shader_body
comp_2=`{
comp_3=`  float4 ret = float4(0.0);
comp_4=`}
"#;
        let doc = parse_milk(src, "backtick_shader").expect("v2 preset parses");

        assert!(
            !doc.warp.contains('`'),
            "assembled warp must carry no backticks, got {:?}",
            doc.warp
        );
        assert!(
            !doc.comp.contains('`'),
            "assembled comp must carry no backticks, got {:?}",
            doc.comp
        );

        let expected_warp = concat!(
            "float4 helper(float2 p) {\n",
            "  return float4(p, 0.0, 1.0);\n",
            "}\n",
            "shader_body\n",
            "{\n",
            "  float3 ret = helper(uv_orig).rgb;\n",
            "}",
        );
        assert_eq!(
            doc.warp, expected_warp,
            "one leading backtick per line stripped, indentation and braces kept"
        );

        let shader = extract_shader_body(&doc.warp);
        assert!(
            shader.prelude.contains("float4 helper(float2 p) {"),
            "prelude keeps the helper declaration, got {:?}",
            shader.prelude
        );
        assert!(
            shader.prelude.contains("  return float4(p, 0.0, 1.0);"),
            "prelude keeps original indentation, got {:?}",
            shader.prelude
        );
        assert!(
            shader.prelude.contains("}\n"),
            "helper closing brace stays intact, got {:?}",
            shader.prelude
        );
        assert!(
            shader.body.contains("float3 ret = helper(uv_orig).rgb;"),
            "body keeps the helper call site, got {:?}",
            shader.body
        );
        assert!(
            !shader.prelude.contains('`') && !shader.body.contains('`'),
            "extracted prelude/body carry no backticks"
        );
    }

    /// Escape stripping removes exactly ONE leading backtick per value line:
    /// indentation and any later (mid-line) backticks must survive untouched.
    #[test]
    fn only_one_leading_backtick_is_stripped_from_equation_values() {
        let src = r#"[preset00]
per_frame_1=`x=1; y=`z;
per_frame_2=``x=2;
per_pixel_1=`  q = 1;
"#;
        let doc = parse_milk(src, "backtick_eqs").expect("parses");
        assert_eq!(
            doc.frame_eqs, "x=1; y=`z;\n`x=2;",
            "strip one leading backtick per line; keep mid-line and second backticks"
        );
        assert_eq!(
            doc.pixel_eqs, "  q = 1;",
            "leading indentation after the stripped backtick is preserved"
        );
    }

    /// A UTF-8 BOM in front of `[preset00]` is a common `.milk` artefact and
    /// must be stripped before section detection; a BOM alone is not a section.
    #[test]
    fn utf8_bom_before_preset_section_is_accepted() {
        let src = "\u{feff}[preset00]\nfDecay=0.9\nper_frame_1=x=1;\n";
        let doc = parse_milk(src, "bom").expect("BOM-prefixed preset parses");
        assert_eq!(doc.version, 1);
        assert!(
            (doc.base.decay - 0.9).abs() < 1e-9,
            "base vals after the BOM section still parse"
        );
        assert_eq!(doc.frame_eqs, "x=1;");

        let no_section = "\u{feff}fDecay=0.9\nper_frame_1=x=1;\n";
        assert!(
            matches!(
                parse_milk(no_section, "bom_no_section"),
                Err(MilkParseError::PresetSectionMissing)
            ),
            "a BOM does not turn a section-less file into a preset"
        );
    }

    /// `psversion*` are real v2 metadata keys: recognised-but-ignored, exactly
    /// like the already-known rating/shader/preview keys. Genuine unknowns stay
    /// in diagnostics.
    #[test]
    fn real_v2_metadata_keys_are_recognised_but_ignored() {
        let src = r#"[preset00]
psversion=3
psversion_warp=2
psversion_comp=2
frating=5.0
fshader=2
fpreview=1
fDecay=0.9
per_frame_1=x=1;
"#;
        let doc = parse_milk(src, "metadata").expect("parses");
        assert!(
            doc.diagnostics.is_empty(),
            "real v2 metadata keys must not become diagnostics, got {:?}",
            doc.diagnostics
        );

        let control = "[preset00]\npsversion=3\nfTotallyUnknownKey=1\n";
        let control_doc = parse_milk(control, "control").expect("parses");
        assert!(
            control_doc
                .diagnostics
                .iter()
                .any(|d| d.to_lowercase().contains("totallyunknownkey")),
            "a genuinely unknown key is still diagnosed, got {:?}",
            control_doc.diagnostics
        );
    }

    /// A representative real v2 header: the compatibility rename table maps
    /// these names onto canonical base vals; omitted fields keep defaults.
    #[test]
    fn real_v2_header_renames_map_to_canonical_base_vals() {
        let src = r#"[preset00]
MILKDROP_PRESET_VERSION=201
fDecay=0.98
fGammaAdj=1.5
fVideoEchoZoom=1.25
nWaveMode=8
fWaveAlpha=0.75
fZoomExponent=1.3
nMotionVectorsX=2
nMotionVectorsY=3
"#;
        let doc = parse_milk(src, "header").expect("parses");
        assert_eq!(doc.version, 2);
        assert!((doc.base.decay - 0.98).abs() < 1e-9, "fDecay -> decay");
        assert!(
            (doc.base.gammaadj - 1.5).abs() < 1e-9,
            "fGammaAdj -> gammaadj"
        );
        assert!(
            (doc.base.echo_zoom - 1.25).abs() < 1e-9,
            "fVideoEchoZoom -> echo_zoom"
        );
        assert_eq!(doc.base.wave_mode, 8, "nWaveMode -> wave_mode");
        assert!(
            (doc.base.wave_a - 0.75).abs() < 1e-9,
            "fWaveAlpha -> wave_a"
        );
        assert!(
            (doc.base.zoomexp - 1.3).abs() < 1e-9,
            "fZoomExponent -> zoomexp"
        );
        assert_eq!(doc.base.mv_x, 2, "nMotionVectorsX -> mv_x");
        assert_eq!(doc.base.mv_y, 3, "nMotionVectorsY -> mv_y");
        // Canonical defaults for omitted fields.
        assert!((doc.base.zoom - 1.0).abs() < 1e-9, "default zoom");
        assert!((doc.base.warp - 1.0).abs() < 1e-9, "default warp");
        assert!((doc.base.cx - 0.5).abs() < 1e-9, "default cx");
        assert!(
            (doc.base.brightness - 1.0).abs() < 1e-9,
            "default brightness"
        );
    }

    /// `[preset00]` detection is case-insensitive, tolerates surrounding
    /// whitespace, and CRLF line endings are still normalised to LF.
    #[test]
    fn preset_section_case_whitespace_and_crlf_are_tolerated() {
        let src = "  [PrEsEt00]  \r\nfDecay=0.9\r\nper_frame_1=x=1;\r\n";
        let doc = parse_milk(src, "case").expect("parses");
        assert!(
            (doc.base.decay - 0.9).abs() < 1e-9,
            "mixed-case section with padding is found"
        );
        assert_eq!(doc.frame_eqs, "x=1;");
        assert!(!doc.frame_eqs.contains('\r'), "CRLF normalised to LF");
    }

    /// Real wave/shape sections: `*_enabled` gates activity, equations are
    /// backtick-escaped, and `shape_*_per_point` is not valid but must be
    /// ignored gracefully rather than diagnosed or panicking.
    #[test]
    fn real_wave_and_shape_sections_enable_and_strip_backticks() {
        let src = r#"[preset00]
wavecode_1_enabled=1
wavecode_1_r=1.0
wavecode_2_enabled=0
wave_1_per_frame_1=`x=1;
shapecode_1_enabled=1
shapecode_2_enabled=0
shape_1_per_frame_1=`shape_x = 0.5;
shape_1_per_point_1=`ignored = 1;
"#;
        let doc = parse_milk(src, "wave_shape").expect("parses");

        assert_eq!(doc.active_waves().len(), 1, "one enabled wave");
        assert_eq!(doc.active_shapes().len(), 1, "one enabled shape");

        let wave = doc.active_waves()[0];
        let shape = doc.active_shapes()[0];
        assert_eq!(
            wave.frame_eqs, "x=1;",
            "wave equation has no leading backtick, got {:?}",
            wave.frame_eqs
        );
        assert_eq!(
            shape.frame_eqs, "shape_x = 0.5;",
            "shape equation has no leading backtick, got {:?}",
            shape.frame_eqs
        );
        assert!(
            !doc.diagnostics
                .iter()
                .any(|d| d.to_lowercase().contains("per_point")),
            "shape per_point is ignored gracefully, got {:?}",
            doc.diagnostics
        );
    }

    /// Real v1 presets carry a bare `warp=<scalar>` / `comp=<scalar>` base val.
    /// `numbered_index` currently maps a bare key equal to the prefix to index
    /// 0, so the scalar leaks into `warp_lines[0]`/`comp_lines[0]` and becomes
    /// the first line of the assembled shader source (invalid GLSL).
    #[test]
    fn warp_and_comp_base_val_scalars_do_not_become_shader_source() {
        let v1 = "[preset00]\nwarp=0.01\ncomp=0.02\nper_frame_1=x=1;\n";
        let doc = parse_milk(v1, "v1_bare_scalars").expect("v1 preset parses");

        assert!(
            doc.warp.is_empty(),
            "bare warp=<scalar> is a base val, not shader source, got {:?}",
            doc.warp
        );
        assert!(
            doc.comp.is_empty(),
            "bare comp=<scalar> is a base val, not shader source, got {:?}",
            doc.comp
        );
        assert_eq!(doc.frame_eqs, "x=1;", "unrelated equations still parse");

        // Real v2 presets have both a bare scalar and numbered shader lines.
        let v2 = concat!(
            "[preset00]\n",
            "MILKDROP_PRESET_VERSION=201\n",
            "warp=0.0\n",
            "warp_1=shader_body\n",
            "warp_2={\n",
            "warp_3=  ret = float3(1.0);\n",
            "warp_4=}\n",
        );
        let doc2 = parse_milk(v2, "v2_warp_scalar").expect("v2 preset parses");
        assert!(
            doc2.warp.contains("shader_body"),
            "numbered warp lines must survive, got {:?}",
            doc2.warp
        );
        assert!(
            !doc2.warp.lines().any(|line| line.trim() == "0.0"),
            "the bare scalar must not become a bare source line, got {:?}",
            doc2.warp
        );
        assert!(
            doc2.warp.starts_with("shader_body"),
            "assembled warp must start with the first numbered line, got {:?}",
            doc2.warp
        );
    }

    /// Real v2 preset `Bioluminescent Abyss.milk` has `wavecode_0_*` keys.
    /// `section_index("0")` eagerly evaluates `i - 1` in `.then_some(...)`,
    /// underflowing in debug builds. Index 0 is invalid (1-based sections) but
    /// must be ignored gracefully, never panic.
    #[test]
    fn zero_indexed_wavecode_and_shapecode_do_not_panic() {
        let src = concat!(
            "[preset00]\n",
            "wavecode_0_enabled=1\n",
            "wavecode_0_r=1.0\n",
            "shapecode_0_enabled=1\n",
            "wavecode_1_enabled=1\n",
            "shapecode_1_enabled=1\n",
        );

        let result = std::panic::catch_unwind(|| parse_milk(src, "zero_index"));
        let doc = match result {
            Ok(doc) => doc.expect("zero-indexed keys must still parse"),
            Err(_) => panic!("parse_milk must not panic on 0-indexed wavecode/shapecode keys"),
        };

        assert_eq!(
            doc.active_waves().len(),
            1,
            "only the valid wave 1 section is active"
        );
        assert_eq!(
            doc.active_shapes().len(),
            1,
            "only the valid shape 1 section is active"
        );
    }
}
