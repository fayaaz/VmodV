//! MilkDrop warp/comp shader → WGSL translation via naga (Layer 3).
//!
//! Mirrors [`crate::isf::translate`]: a `MilkDocument` shader body is wrapped in
//! a complete desktop GLSL 450 shader (uniform prelude + HLSL compatibility
//! macros + the body), fed through naga's GLSL front-end, validated, and
//! written out as WGSL. The result is owned so a wgpu pipeline builder can take
//! it without borrowing the parsed preset.
//!
//! # Pipeline
//!
//! ```text
//! prelude + body ──[wrap_milk_shader]──▶ desktop GLSL 450
//!                                        │
//!                  naga::front::glsl (stage = Fragment, __VERSION__ = 450)
//!                                        ▼
//!                          naga::valid::Validator (all flags/capabilities)
//!                                        ▼
//!                      naga::back::wgsl (WriterFlags::empty)
//!                                        ▼
//!                                   WGSL
//! ```
//!
//! # Runtime binding contract
//!
//! The generated WGSL is consumed by the Layer 5 wgpu pipeline builder:
//!
//! * `@group(0) @binding(0)` — a uniform buffer holding the MilkDrop prelude,
//!   padded to WGSL's 16-byte alignment (256-byte stride if dynamic offsets are
//!   used).
//! * Textures/samplers follow in fixed order: built-ins first
//!   (`sampler_main`, `sampler_fw_main`, `sampler_pw_main`, `sampler_fc_main`,
//!   `sampler_pc_main`, noise/blur samplers), then user samplers in declaration
//!   order.
//! * Entry point `main` returns `@location(0) vec4<f32>`; the full-screen
//!   triangle drives comp, while warp consumes the mesh's warped UVs.
//!
//! # Real MilkDrop 2 conventions bridged here
//!
//! Production v2 presets put more than uniform declarations in the `prelude`:
//!
//! * **Helper functions** (HLSL `float2/3/4` signatures and bodies) that
//!   `shader_body` calls. These are retained by `retained_prelude` with HLSL
//!   vector types rewritten to GLSL.
//! * **`static` globals** — legal in MilkDrop's HLSL-flavoured prelude but not
//!   at GLSL 450 global scope, so the qualifier is stripped while the
//!   declaration survives.
//! * **HLSL builtins** `lerp`/`saturate`/`frac`, defined in `HLSL_MACROS` as
//!   `mix`/`clamp`/`fract`.
//! * **`ret` of either width** — v2 bodies declare `float3 ret` or
//!   `float4 ret`; [`wrap_milk_shader`] writes `vec4(ret.xyz, 1.0)` so both
//!   compile.
//!
//! Uniforms and samplers are owned by `uniform_block_glsl` /
//! `sampler_bindings_glsl`; `retained_prelude` skips those top-level
//! declarations (plus reserved sampler redeclarations) so nothing is emitted
//! twice, and tracks brace depth so lines *inside* helper bodies are never
//! mistaken for them.

#![forbid(unsafe_code)]

use std::fmt;

use crate::isf::translate::glsl_to_wgsl;

/// Which MilkDrop shader stage is being wrapped/translated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MilkShaderStage {
    /// The warp shader — produces the warped UVs sampled by the comp pass.
    Warp,
    /// The composite shader — produces the final pixel colour.
    Comp,
}

/// An owned, translation result for one MilkDrop shader stage.
///
/// Owns every string so the caller can build a wgpu pipeline without keeping
/// the parsed `MilkDocument` alive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledMilkShader {
    /// The generated WGSL, ready for `wgpu::ShaderModuleDescriptor`.
    pub wgsl: String,
    /// The complete desktop-GLSL 450 source fed to naga (prelude + macros +
    /// `body` + the `main` wrapper).
    pub source: String,
    /// Which stage produced this module.
    pub stage: MilkShaderStage,
}

/// A typed translation failure that keeps the offending source and the naga
/// parse/validate diagnostics instead of silently falling back to a black
/// shader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslateError {
    /// The source that failed to translate (wrapped GLSL plus the original
    /// body, so callers can point at the offending line).
    pub source: String,
    /// naga's parse and/or validation diagnostics.
    pub diagnostics: String,
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MilkDrop shader translation failed: {}\n--- source ---\n{}",
            self.diagnostics, self.source
        )
    }
}

impl std::error::Error for TranslateError {}

/// Built-in sampler uniforms, in the fixed order the runtime binds them.
///
/// These are always declared by the wrapper (as split `texture2D`/`sampler`
/// pairs, mirroring ISF's `texture_declarations`) so MilkDrop bodies can call
/// `GetMain`, `GetBlur1`, … without declaring anything themselves.
const BUILTIN_SAMPLERS: &[&str] = &[
    "sampler_main",
    "sampler_fw_main",
    "sampler_pw_main",
    "sampler_fc_main",
    "sampler_pc_main",
    "sampler_blur1",
    "sampler_blur2",
    "sampler_blur3",
    "sampler_noise_lq",
    "sampler_noise_mq",
    "sampler_noise_hq",
    "sampler_pw_noise_lq",
    "sampler_pw_noise_hq",
];

/// Noise samplers the runtime binds positionally after the eight original
/// built-ins. A preset that redeclares one of these in its prelude is stripped
/// defensively so the split binding emitted below is the single declaration
/// naga sees.
const NOISE_SAMPLERS: &[&str] = &[
    "sampler_noise_lq",
    "sampler_noise_mq",
    "sampler_noise_hq",
    "sampler_pw_noise_lq",
    "sampler_pw_noise_hq",
];

/// The canonical MilkDrop uniform members emitted into the `@group(0)
/// @binding(0)` block. Scalars are `float`, coordinates/`texsize` are `vec2`.
/// Every name a body can reference (per the runtime prelude) is present here.
const UNIFORM_MEMBERS: &[(&str, &str)] = &[
    ("float", "time"),
    ("float", "frame"),
    ("float", "fps"),
    ("float", "progress"),
    ("float", "decay"),
    ("float", "bass"),
    ("float", "mid"),
    ("float", "treb"),
    ("float", "bass_att"),
    ("float", "mid_att"),
    ("float", "treb_att"),
    ("float", "vol"),
    ("float", "rand_preset"),
    ("float", "rand_frame"),
    ("float", "aspect"),
    ("float", "slow_roam_cos"),
    ("float", "slow_roam_sin"),
    ("float", "roam_cos"),
    ("float", "roam_sin"),
    ("float", "hue_shader"),
    ("float", "scale1"),
    ("float", "scale2"),
    ("float", "scale3"),
    ("float", "bias1"),
    ("float", "bias2"),
    ("float", "bias3"),
    ("float", "blur1_min"),
    ("float", "blur1_max"),
    ("float", "blur2_min"),
    ("float", "blur2_max"),
    ("float", "blur3_min"),
    ("float", "blur3_max"),
    ("vec2", "texsize"),
    ("vec2", "texsize_fw"),
    ("vec2", "texsize_pw"),
    ("vec2", "texsize_fc"),
    ("vec2", "texsize_pc"),
    // Kept as a uniform slot so the byte layout stays stable; shader bodies
    // read the per-pixel `uv_orig` varying declared by `wrap_milk_shader`.
    ("vec2", "uv_orig_u"),
    ("vec2", "rad"),
    ("vec2", "ang"),
];

/// GLSL definitions of the HLSL/MilkDrop compatibility shorthands.
///
/// Samplers are declared as object-like macros expanding to naga's
/// `sampler2D(texture, sampler)` constructor (see [`sampler_bindings_glsl`]), so
/// the function-like sampling macros work unchanged. All of these are valid for
/// naga's preprocessor (no `##` token pasting — `pp-rs` does not support it).
const HLSL_MACROS: &str = "\
#define tex2D(s, uv) texture(s, uv)
#define tex3D(s, uv) texture(s, uv)
#define GetMain(uv) texture(sampler_main, uv)
#define GetPixel(uv) texture(sampler_fw_main, uv)
#define GetBlur1(uv) texture(sampler_blur1, uv)
#define GetBlur2(uv) texture(sampler_blur2, uv)
#define GetBlur3(uv) texture(sampler_blur3, uv)
#define lum(c) dot((c).rgb, vec3(0.2126, 0.7152, 0.0722))
#define lerp mix
#define saturate(x) clamp((x), 0.0, 1.0)
#define frac fract
#define M_PI 3.14159265358979323846
#define M_PI_2 1.57079632679489661923
#define M_INV_PI_2 0.63661977236758134308
";

// --- Body rewriting --------------------------------------------------------

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// True when `needle` occurs in `source` as a whole identifier, i.e. not as a
/// substring of a longer identifier. Shared by the sampler-usage scan below and
/// by reserved-redeclaration detection so both agree on what "mentions a
/// sampler" means.
fn contains_ident(source: &str, needle: &str) -> bool {
    let mut rest = source;
    while let Some(index) = rest.find(needle) {
        let before_ok = rest[..index]
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident_char(c));
        let after = &rest[index + needle.len()..];
        let after_ok = after.chars().next().is_none_or(|c| !is_ident_char(c));
        if before_ok && after_ok {
            return true;
        }
        rest = after;
    }
    false
}

/// Replace whole-identifier occurrences of `from` with `to` in `source`.
fn replace_ident(source: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(position) = rest.find(from) {
        let (before, after_start) = rest.split_at(position);
        let after = &after_start[from.len()..];
        let before_ok = before.chars().next_back().is_none_or(|c| !is_ident_char(c));
        let after_ok = after.chars().next().is_none_or(|c| !is_ident_char(c));
        out.push_str(before);
        out.push_str(if before_ok && after_ok { to } else { from });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Rewrite HLSL `float2/3/4` constructors and declarations to GLSL vectors.
fn rewrite_hlsl_types(body: &str) -> String {
    let body = replace_ident(body, "float2", "vec2");
    let body = replace_ident(&body, "float3", "vec3");
    replace_ident(&body, "float4", "vec4")
}

// --- Prelude parsing -------------------------------------------------------

/// Strip a leading `qualifier` that is followed by whitespace, returning the
/// trimmed remainder. The whitespace check avoids matching prefixes of longer
/// identifiers (e.g. `highp` in `highpressure`).
fn strip_qualifier<'a>(source: &'a str, qualifier: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(qualifier)?;
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Every `(type, name)` uniform member declared on a single prelude line.
///
/// Handles multiple declarators (`uniform float a, b;`), a leading
/// `layout(...)`, precision qualifiers (`highp`/`mediump`/`lowp`) and trailing
/// `//` comments. Array/initializer suffixes are dropped from the name.
/// Non-uniform lines yield nothing, so comments and blank lines are ignored.
fn parse_uniform_line(line: &str) -> Vec<(&str, &str)> {
    let line = line.split("//").next().unwrap_or(line).trim();

    let after_layout = if let Some(rest) = line.strip_prefix("layout") {
        let rest = rest.trim_start();
        let Some(end) = rest.strip_prefix('(').and_then(|r| r.find(')')) else {
            return Vec::new();
        };
        rest[end + 1..].trim()
    } else {
        line
    };

    let Some(after_uniform) = after_layout.strip_prefix("uniform") else {
        return Vec::new();
    };
    if !after_uniform.is_empty() && !after_uniform.starts_with(|c: char| c.is_whitespace()) {
        return Vec::new();
    }

    let mut rest = after_uniform.trim_start();
    for qualifier in ["highp", "mediump", "lowp", "flat", "smooth"] {
        if let Some(next) = strip_qualifier(rest, qualifier) {
            rest = next;
        }
    }

    let declaration = rest.split(';').next().unwrap_or(rest);
    let mut parts = declaration.splitn(2, |c: char| c.is_whitespace());
    let ty = parts.next().unwrap_or("").trim();
    let declarators = parts.next().unwrap_or("").trim();
    if ty.is_empty() || declarators.is_empty() {
        return Vec::new();
    }

    declarators
        .split(',')
        .filter_map(|decl| {
            let name = decl.trim().split(['=', '[']).next()?.trim();
            (!name.is_empty()).then_some((ty, name))
        })
        .collect()
}

/// Every uniform member declared anywhere in `prelude`, in source order.
fn prelude_uniforms(prelude: &str) -> impl Iterator<Item = (&str, &str)> + '_ {
    prelude.lines().flat_map(parse_uniform_line)
}

/// True when `name` is bound unconditionally by the wrapper (a built-in or a
/// positional noise sampler) and must not be redeclared from the prelude.
fn is_reserved_sampler(name: &str) -> bool {
    BUILTIN_SAMPLERS.contains(&name) || NOISE_SAMPLERS.contains(&name)
}

/// One wrapper-owned sampler binding. `name` is `None` for a built-in noise
/// slot the body never references: the slot is still declared with an
/// anonymous name so every sampler keeps its fixed `BUILTIN_SAMPLERS` position
/// (and the runtime's matching bind-group slot) without leaking the reserved
/// identifier into the generated shader. See [`sampler_bindings_glsl`].
struct SamplerBinding {
    name: Option<String>,
}

/// The samplers a wrapper must bind: the 13 built-ins first (in fixed order),
/// then user samplers from the prelude in declaration order. Reserved/noise
/// samplers are skipped as user declarations so the wrapper stays the single
/// declaration naga sees. A noise sampler the body never uses is bound
/// anonymously at its reserved slot, keeping binding indices stable.
///
/// `retained` is the prelude text that actually survives into the shader (see
/// [`retained_prelude`]); passing it in lets a noise sampler referenced only
/// from a prelude helper earn a named binding instead of an anonymous slot the
/// helper cannot refer to. The raw `prelude` is still needed for user sampler
/// declarations.
fn samplers_for_prelude(prelude: &str, retained: &str, body: &str) -> Vec<SamplerBinding> {
    let mut samplers: Vec<SamplerBinding> = BUILTIN_SAMPLERS
        .iter()
        .map(|name| SamplerBinding {
            name: (!NOISE_SAMPLERS.contains(name)
                || contains_ident(body, name)
                || contains_ident(retained, name))
            .then(|| name.to_string()),
        })
        .collect();
    for (ty, name) in prelude_uniforms(prelude) {
        if ty != "sampler2D" || is_reserved_sampler(name) {
            continue;
        }
        if samplers
            .iter()
            .any(|existing| existing.name.as_deref() == Some(name))
        {
            continue;
        }
        samplers.push(SamplerBinding {
            name: Some(name.to_string()),
        });
    }
    samplers
}

/// Split `texture2D`/`sampler` bindings plus object-like macros aliasing each
/// sampler name to naga's `sampler2D(texture, sampler)` constructor.
///
/// Binding 0 is the uniform block, so textures/samplers occupy 1, 2, 3, …
/// Named bindings emit the alias macro; anonymous ones (unused noise slots)
/// only declare the `texture`/`sampler` pair, so the slot stays bound at a
/// fixed index without the reserved sampler name appearing in the shader.
fn sampler_bindings_glsl(samplers: &[SamplerBinding]) -> String {
    let mut out = String::new();
    for (index, binding) in samplers.iter().enumerate() {
        let texture_binding = (index as u32) * 2 + 1;
        let sampler_binding = texture_binding + 1;
        match &binding.name {
            Some(name) => {
                out.push_str(&format!(
                    "#define {name} sampler2D(isf_tex_{name}, isf_smp_{name})\n"
                ));
                out.push_str(&format!(
                    "layout(binding = {texture_binding}) uniform texture2D isf_tex_{name};\n"
                ));
                out.push_str(&format!(
                    "layout(binding = {sampler_binding}) uniform sampler isf_smp_{name};\n"
                ));
            }
            None => {
                out.push_str(&format!(
                    "layout(binding = {texture_binding}) uniform texture2D isf_tex_unused_{index};\n"
                ));
                out.push_str(&format!(
                    "layout(binding = {sampler_binding}) uniform sampler isf_smp_unused_{index};\n"
                ));
            }
        }
    }
    out.push('\n');
    out
}

/// The uniform block: canonical members plus any scalar/vector uniform declared
/// in the prelude that is not already covered. Sampler declarations (including
/// reserved noise samplers) are skipped — they become split bindings or are
/// bound by the wrapper instead.
fn uniform_block_glsl(prelude: &str) -> String {
    let mut members: Vec<(&str, &str)> = UNIFORM_MEMBERS.to_vec();
    for (ty, name) in prelude_uniforms(prelude) {
        if ty == "sampler2D" || is_reserved_sampler(name) {
            continue;
        }
        if !members.iter().any(|(_, existing)| *existing == name) {
            members.push((ty, name));
        }
    }
    let mut out = String::from("layout(binding = 0) uniform MilkUniforms {\n");
    for (ty, name) in members {
        out.push_str(&format!("  {ty} {name};\n"));
    }
    out.push_str("};\n\n");
    out
}

/// Net brace depth change of a line, ignoring `//` comments.
///
/// Prelude helpers can open a body on one line and close it on another; callers
/// use the running total to tell top-level declarations from lines *inside* a
/// helper body.
fn brace_delta(line: &str) -> i64 {
    let code = line.split("//").next().unwrap_or(line);
    code.chars().fold(0, |depth, c| match c {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

/// True when a prelude line merely declares (rather than uses) a sampler the
/// wrapper already binds — a built-in or a positional noise sampler. Such a
/// redeclaration would expand into a duplicate binding and break naga.
///
/// Deliberately conservative: it needs a trailing `;` and no `(`, so a call or
/// a use site with parentheses is never mistaken for a declaration. Callers
/// apply it only at top level (see [`retained_prelude`]) so a bare use site in a
/// helper body is kept too.
fn declares_reserved_sampler(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.ends_with(';') || trimmed.contains('(') {
        return false;
    }
    let lower = trimmed.to_lowercase();
    BUILTIN_SAMPLERS
        .iter()
        .chain(NOISE_SAMPLERS.iter())
        .any(|name| contains_ident(&lower, name))
}

/// The non-uniform prelude lines that must appear before `main`: helper
/// functions, `static` globals and other declarations. Uniform declarations are
/// folded into the uniform block / sampler bindings above, and reserved sampler
/// redeclarations are dropped, so neither is emitted twice.
///
/// Only *top-level* lines (brace depth zero) can be wrapper-owned declarations.
/// Lines inside a helper body are always kept verbatim: a local declaration or
/// a use site such as `foo = sampler_pw_noise_lq;` must not be mistaken for a
/// uniform/sampler redeclaration and dropped.
///
/// HLSL vector types are rewritten to GLSL and the HLSL-only `static` storage
/// qualifier is stripped (it is illegal at GLSL 450 global scope).
fn retained_prelude(prelude: &str) -> String {
    let mut out = String::new();
    let mut depth: i64 = 0;
    for line in prelude.lines() {
        let trimmed = line.trim();
        let at_top_level = depth == 0;

        if !trimmed.is_empty()
            && !(at_top_level && !parse_uniform_line(line).is_empty())
            && !(at_top_level && declares_reserved_sampler(line))
        {
            let indent_len = line.len() - trimmed.len();
            let content = strip_qualifier(trimmed, "static").unwrap_or(trimmed);
            out.push_str(&line[..indent_len]);
            out.push_str(content);
            out.push('\n');
        }

        // Update after emitting: the opening line of a helper is still
        // top-level, but everything up to and including its closing brace is
        // shielded by the new depth.
        depth = (depth + brace_delta(line)).max(0);
    }
    rewrite_hlsl_types(&out)
}

/// Wrap a MilkDrop `prelude`/`body` pair in a complete desktop-GLSL 450 shader.
///
/// Emits the MilkDrop uniform prelude, HLSL compatibility macros, the retained
/// prelude declarations, the body, and a `main` entry point returning
/// `vec4(ret.xyz, 1.0)` (component-wise so both `float3` and `float4` bodies
/// work).
pub fn wrap_milk_shader(prelude: &str, body: &str, stage: MilkShaderStage) -> String {
    // Compute the retained prelude once: it is both emitted before `main` and
    // scanned for noise-sampler use sites inside helper functions. Scanning the
    // retained (rather than raw) text keeps top-level declarations from earning
    // a named binding while preserving use sites inside helper bodies.
    let retained = retained_prelude(prelude);
    let samplers = samplers_for_prelude(prelude, &retained, body);
    let body = rewrite_hlsl_types(body);

    let mut out = String::with_capacity(body.len() + 4096 + retained.len());
    out.push_str("#version 450\n\n");
    out.push_str(&format!("// MilkDrop {stage:?} shader\n"));
    out.push_str(HLSL_MACROS);
    out.push('\n');
    out.push_str(&uniform_block_glsl(prelude));
    // `uv_orig` is a per-pixel varying from the full-screen vertex stage
    // (`FULLSCREEN_VERTEX_WGSL`, @location(0)). Both warp and comp are fragment
    // stages, so this single declaration serves them and makes texture samples
    // vary across the frame. The uniform block keeps an `uv_orig_u` slot at the
    // same byte offset purely for layout stability; bodies read the varying.
    out.push_str("layout(location = 0) in vec2 uv_orig;\n");
    out.push_str(&sampler_bindings_glsl(&samplers));
    out.push_str("layout(location = 0) out vec4 milk_FragColor;\n\n");
    out.push_str(&retained);
    out.push_str("void main() {\n");
    out.push_str(&body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("  milk_FragColor = vec4(ret.xyz, 1.0);\n");
    out.push_str("}\n");
    out
}

/// Translate one MilkDrop shader stage into owned WGSL.
///
/// Both stages are fragment shaders: warp produces warped UVs, comp produces
/// the final colour. naga setup is shared with the ISF path via
/// [`crate::isf::translate::glsl_to_wgsl`].
pub fn translate_milk_shader(
    prelude: &str,
    body: &str,
    stage: MilkShaderStage,
) -> Result<CompiledMilkShader, TranslateError> {
    let source = wrap_milk_shader(prelude, body, stage);
    let wgsl =
        glsl_to_wgsl(&source, naga::ShaderStage::Fragment).map_err(|error| TranslateError {
            source: source.clone(),
            diagnostics: error.to_string(),
        })?;
    Ok(CompiledMilkShader {
        wgsl,
        source,
        stage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative warp body: HLSL `float2`/`float3` types, built-in
    /// roam uniforms, a `tex2D` call and an assigned `ret`.
    const WARP_BODY: &str = "\
float2 uv = uv_orig + float2(roam_cos, roam_sin) * 0.01;\n\
float3 ret = tex2D(sampler_main, uv).rgb;\n";

    /// A representative comp body: `GetMain`/`GetBlur1` macros, `time`, and an
    /// accumulated `ret`.
    const COMP_BODY: &str = "\
float2 uv = uv_orig;\n\
uv.x += 0.05 * sin(uv.y * 10.0 + time);\n\
float3 ret = GetMain(uv).rgb;\n\
ret += GetBlur1(uv).rgb * 0.25;\n";

    /// Deliberately broken HLSL: an empty initialiser and garbage tokens. The
    /// GLSL front-end must reject it, and the error must carry the source.
    const MALFORMED_BODY: &str = "float2 uv = ;\nthis is not valid GLSL @@@\nret = uv;\n";

    /// A prelude that declares a user sampler plus the two built-in noise
    /// samplers the runtime binds positionally.
    const PRELUDE_WITH_SAMPLERS: &str = "\
uniform float time;\n\
uniform sampler2D sampler_user_flow;\n\
uniform sampler2D sampler_pw_noise_lq;\n\
uniform sampler2D sampler_pw_noise_hq;\n";

    #[test]
    fn warp_and_comp_bodies_translate_with_main_and_bindings() {
        for (stage, body) in [
            (MilkShaderStage::Warp, WARP_BODY),
            (MilkShaderStage::Comp, COMP_BODY),
        ] {
            let compiled = translate_milk_shader("", body, stage)
                .unwrap_or_else(|error| panic!("{stage:?} body must translate: {error}"));

            assert_eq!(compiled.stage, stage, "the stage is carried through");
            assert!(
                compiled.source.contains("#version 450"),
                "{stage:?}: wrapped GLSL must be desktop 450, got {:?}",
                compiled.source
            );
            assert!(
                compiled.wgsl.contains("fn main"),
                "{stage:?}: generated WGSL must have a `main` entry point, got {:?}",
                compiled.wgsl
            );
            assert!(
                compiled.wgsl.contains("@group(0) @binding(0)"),
                "{stage:?}: uniform must be at group 0 binding 0, got {:?}",
                compiled.wgsl
            );
            assert!(
                compiled.wgsl.contains("texture_2d"),
                "{stage:?}: WGSL must declare a texture binding, got {:?}",
                compiled.wgsl
            );
            assert!(
                compiled.wgsl.contains("sampler"),
                "{stage:?}: WGSL must declare a sampler binding, got {:?}",
                compiled.wgsl
            );
        }
    }

    #[test]
    fn malformed_hlsl_returns_typed_error_with_source_and_diagnostics() {
        let result = translate_milk_shader("", MALFORMED_BODY, MilkShaderStage::Comp);
        assert!(
            result.is_err(),
            "malformed HLSL must produce a typed error, got {result:?}"
        );
        let error = result.expect_err("checked above");
        assert!(
            error.source.contains("this is not valid GLSL"),
            "the error must carry the source that failed, got {:?}",
            error.source
        );
        assert!(
            !error.diagnostics.trim().is_empty(),
            "the error must carry naga diagnostics, got {:?}",
            error.diagnostics
        );
        assert!(
            !error.to_string().trim().is_empty(),
            "the error must implement Display"
        );
    }

    #[test]
    fn user_sampler_is_retained_and_noise_redeclarations_are_removed() {
        let body = "float3 ret = tex2D(sampler_user_flow, uv_orig).rgb;\n";
        let compiled = translate_milk_shader(PRELUDE_WITH_SAMPLERS, body, MilkShaderStage::Comp)
            .unwrap_or_else(|error| panic!("shader with a user sampler must translate: {error}"));

        assert!(
            compiled.wgsl.contains("sampler_user_flow"),
            "user-declared sampler must survive into WGSL, got {:?}",
            compiled.wgsl
        );
        assert!(
            !compiled.wgsl.contains("sampler_pw_noise_lq"),
            "built-in noise redeclarations must be stripped, got {:?}",
            compiled.wgsl
        );
        assert!(
            !compiled.wgsl.contains("sampler_pw_noise_hq"),
            "built-in noise redeclarations must be stripped, got {:?}",
            compiled.wgsl
        );

        let wrapped = wrap_milk_shader(PRELUDE_WITH_SAMPLERS, body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("sampler_user_flow"),
            "wrapper must keep the user sampler, got {wrapped:?}"
        );
        assert!(
            !wrapped.contains("sampler_pw_noise_lq"),
            "wrapper must strip noise redeclarations, got {wrapped:?}"
        );
        assert!(
            !wrapped.contains("sampler_pw_noise_hq"),
            "wrapper must strip noise redeclarations, got {wrapped:?}"
        );
    }

    #[test]
    fn tex2d_and_getmain_macro_usage_translates_to_a_sampling_shader() {
        let body = "\
float2 uv = uv_orig;\n\
float3 ret = tex2D(sampler_main, uv).rgb;\n\
ret += GetMain(uv).rgb;\n";
        let wrapped = wrap_milk_shader("", body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("tex2D"),
            "wrapper must define/expand the tex2D macro, got {wrapped:?}"
        );
        assert!(
            wrapped.contains("GetMain"),
            "wrapper must define/expand the GetMain macro, got {wrapped:?}"
        );

        let compiled = translate_milk_shader("", body, MilkShaderStage::Comp)
            .unwrap_or_else(|error| panic!("tex2D/GetMain body must translate: {error}"));
        assert!(
            compiled.wgsl.contains("textureSample"),
            "tex2D/GetMain must become a real texture sample in WGSL, got {:?}",
            compiled.wgsl
        );
    }

    // --- MilkDrop 2 real-format compatibility (RED) -------------------------

    /// A real v2 shader puts helper functions in the prelude, before
    /// `shader_body`. The wrapper must keep them (rewriting HLSL vector types)
    /// instead of dropping every non-uniform prelude line.
    #[test]
    fn prelude_helper_functions_are_preserved_and_rewritten() {
        let prelude = "\
float4 helper(float2 p) {\n\
  return float4(p, 0.0, 1.0);\n\
}\n";
        let body = "float3 ret = helper(uv_orig).rgb;\n";

        let wrapped = wrap_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("vec4 helper(vec2 p)"),
            "wrapped GLSL must preserve and rewrite the helper signature, got {wrapped:?}"
        );
        assert!(
            wrapped.contains("return vec4(p, 0.0, 1.0);"),
            "wrapped GLSL must preserve and rewrite the helper body, got {wrapped:?}"
        );

        let compiled = translate_milk_shader(prelude, body, MilkShaderStage::Comp)
            .unwrap_or_else(|error| panic!("prelude helper must translate: {error}"));
        assert!(
            compiled.wgsl.contains("helper"),
            "WGSL must preserve the helper function, got {:?}",
            compiled.wgsl
        );
    }

    /// `static` globals are legal in MilkDrop's HLSL-flavoured prelude but not
    /// at GLSL 450 global scope, so the qualifier must be stripped while the
    /// declaration itself is retained.
    #[test]
    fn static_globals_are_preserved_without_static_qualifier() {
        let prelude = "static float yh = 0.66;\n";
        let body = "float3 ret = vec3(yh);\n";

        let wrapped = wrap_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("float yh = 0.66;"),
            "wrapped GLSL must keep the static global declaration, got {wrapped:?}"
        );
        assert!(
            !wrapped.contains("static float yh"),
            "wrapped GLSL must strip the `static` qualifier, got {wrapped:?}"
        );

        let compiled = translate_milk_shader(prelude, body, MilkShaderStage::Comp)
            .unwrap_or_else(|error| panic!("static global prelude must translate: {error}"));
        assert!(
            compiled.wgsl.contains("fn main"),
            "WGSL must have a main entry point, got {:?}",
            compiled.wgsl
        );
    }

    /// HLSL builtins `lerp`/`saturate`/`frac` have no GLSL 450 spelling; the
    /// wrapper must define or rewrite them to `mix`/`clamp`/`fract`.
    #[test]
    fn hlsl_builtins_lerp_saturate_frac_are_rewritten_to_glsl() {
        let body = "\
float a = 0.1;\n\
float b = 0.9;\n\
float t = 0.5;\n\
float x = 0.75;\n\
float3 ret = vec3(lerp(a, b, t), saturate(x), frac(x));\n\
ret += vec3(smoothstep(0.0, 1.0, x), step(0.5, x), 0.0);\n";

        let wrapped = wrap_milk_shader("", body, MilkShaderStage::Comp);
        for expansion in ["mix", "clamp", "fract"] {
            assert!(
                wrapped.contains(expansion),
                "wrapped GLSL must contain HLSL builtin expansion `{expansion}`, got {wrapped:?}"
            );
        }

        let compiled = translate_milk_shader("", body, MilkShaderStage::Comp)
            .unwrap_or_else(|error| panic!("HLSL builtin body must translate: {error}"));
        assert!(
            compiled.wgsl.contains("fn main"),
            "WGSL must have a main entry point, got {:?}",
            compiled.wgsl
        );
    }

    /// Real v2 bodies declare `ret` either as `float4` or `float3`; the final
    /// assignment must accept both via a component-wise `ret.xyz` write.
    #[test]
    fn float4_and_float3_ret_both_translate_with_component_wise_assignment() {
        let float4_body = "float4 ret = float4(0.0);\n";
        let float3_body = "float3 ret = vec3(0.0);\n";

        let wrapped = wrap_milk_shader("", float4_body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("milk_FragColor = vec4(ret.xyz, 1.0);"),
            "wrapper final assignment must accept both float3 and float4 ret, got {wrapped:?}"
        );

        for (body, label) in [(float4_body, "float4"), (float3_body, "float3")] {
            translate_milk_shader("", body, MilkShaderStage::Comp)
                .unwrap_or_else(|error| panic!("{label} ret body must translate: {error}"));
        }
    }

    /// Regression fixture mirroring the structure of a real v2 warp preset:
    /// a multi-statement prelude helper with HLSL vector types, a `static`
    /// global, HLSL builtins in the body, `tex2D` sampling and a `float3 ret`.
    #[test]
    fn combined_real_world_warp_prelude_translates_with_helper_and_sampling() {
        let prelude = "\
static float yh = 0.66;\n\
float4 helper(float2 p) {\n\
  float2 q = p * 3.0;\n\
  q = q - floor(q);\n\
  return float4(q, 0.0, 1.0);\n\
}\n";
        let body = "\
float2 uv = uv_orig;\n\
float3 t = helper(uv).rgb;\n\
float3 ret = lerp(t, tex2D(sampler_main, uv).rgb, 0.5);\n\
ret = saturate(ret) * frac(t);\n";

        let compiled = translate_milk_shader(prelude, body, MilkShaderStage::Warp)
            .unwrap_or_else(|error| panic!("combined warp fixture must translate: {error}"));
        assert!(
            compiled.wgsl.contains("fn helper"),
            "WGSL must preserve the prelude helper function, got {:?}",
            compiled.wgsl
        );
        assert!(
            compiled.wgsl.contains("textureSample"),
            "tex2D must become a textureSample in WGSL, got {:?}",
            compiled.wgsl
        );
    }

    // --- MilkDrop 2 built-in noise samplers (RED) ---------------------------

    /// A real v2 comp shader that samples all five built-in noise textures by
    /// name. The wrapper must declare each one as a split texture/sampler pair
    /// so naga sees a valid identifier.
    const NOISE_SAMPLER_BODY: &str = "\
vec2 uv = uv_orig;\n\
float3 ret = tex2D(sampler_noise_lq, uv).rgb;\n\
ret += tex2D(sampler_noise_mq, uv).rgb;\n\
ret += tex2D(sampler_noise_hq, uv).rgb;\n\
ret += tex2D(sampler_pw_noise_lq, uv).rgb;\n\
ret += tex2D(sampler_pw_noise_hq, uv).rgb;\n";

    #[test]
    fn noise_samplers_are_declared_and_sampled_in_wgsl() {
        let wrapped = wrap_milk_shader("", NOISE_SAMPLER_BODY, MilkShaderStage::Comp);
        for name in [
            "sampler_noise_lq",
            "sampler_noise_mq",
            "sampler_noise_hq",
            "sampler_pw_noise_lq",
            "sampler_pw_noise_hq",
        ] {
            assert!(
                wrapped.contains(&format!("isf_tex_{name}")),
                "wrapped GLSL must declare a texture binding for {name}, got {wrapped:?}"
            );
            assert!(
                wrapped.contains(&format!("isf_smp_{name}")),
                "wrapped GLSL must declare a sampler binding for {name}, got {wrapped:?}"
            );
        }

        let compiled = translate_milk_shader("", NOISE_SAMPLER_BODY, MilkShaderStage::Comp)
            .unwrap_or_else(|error| {
                panic!("comp body sampling all five noise samplers must translate: {error}")
            });
        assert!(
            compiled.wgsl.contains("textureSample"),
            "noise sampler tex2D calls must become textureSample in WGSL, got {:?}",
            compiled.wgsl
        );
    }

    /// A noise sampler referenced only inside a prelude helper (not in the
    /// `shader_body`) must still get a real named binding. Otherwise the wrapper
    /// binds that slot anonymously (`isf_tex_unused_N`) and naga fails with
    /// "Unknown variable: sampler_noise_lq" when the helper calls `tex2D`.
    #[test]
    fn samplers_used_only_in_prelude_helpers_get_named_bindings() {
        let prelude = "\
float4 helper(float2 p) {\n\
  return tex2D(sampler_noise_lq, p);\n\
}\n";
        let body = "float3 ret = helper(uv_orig).rgb;\n";

        let wrapped = wrap_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            wrapped.contains("isf_tex_sampler_noise_lq"),
            "a sampler used only inside a prelude helper must get a named texture binding, got {wrapped:?}"
        );
        assert!(
            wrapped.contains("isf_smp_sampler_noise_lq"),
            "a sampler used only inside a prelude helper must get a named sampler binding, got {wrapped:?}"
        );

        let compiled = translate_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            compiled.is_ok(),
            "a shader whose sampler is used only in a prelude helper must translate, got {compiled:?}"
        );
    }

    /// An HLSL-style sampler declaration in the prelude must not leak into the
    /// generated GLSL as an invalid declaration, while the helper body that
    /// uses the same sampler must be retained and still translate.
    #[test]
    fn hlsl_sampler_redeclaration_in_prelude_is_stripped() {
        let prelude = "\
sampler sampler_pw_noise_lq;\n\
float4 helper(float2 p) {\n\
  return tex2D(sampler_pw_noise_lq, p);\n\
}\n";
        let body = "float3 ret = helper(uv_orig).rgb;\n";

        let wrapped = wrap_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            !wrapped.contains("sampler sampler_pw_noise_lq;"),
            "the bare HLSL sampler redeclaration must be stripped from wrapped GLSL, got {wrapped:?}"
        );
        assert!(
            wrapped.contains("tex2D(sampler_pw_noise_lq, p)"),
            "the helper body's use of the sampler must be retained, got {wrapped:?}"
        );

        let compiled = translate_milk_shader(prelude, body, MilkShaderStage::Comp);
        assert!(
            compiled.is_ok(),
            "translation must succeed after the HLSL redeclaration is stripped, got {compiled:?}"
        );
    }

    #[test]
    fn builtin_sampler_count_matches_runtime_binding_pairs() {
        // 8 original built-ins (main, fw, pw, fc, pc, blur1-3) + 5 noise.
        const EXPECTED_BUILTINS: usize = 13;
        assert_eq!(
            BUILTIN_SAMPLERS.len(),
            EXPECTED_BUILTINS,
            "the wrapper must declare all 13 built-in samplers"
        );
        assert_eq!(
            crate::milkdrop::runtime::MILK_RUNTIME_SAMPLER_PAIRS,
            BUILTIN_SAMPLERS.len(),
            "the runtime must bind one (texture, sampler) pair per built-in sampler"
        );
    }
}
