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

#![forbid(unsafe_code)]

use std::fmt;

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

/// Wrap a MilkDrop `prelude`/`body` pair in a complete desktop-GLSL 450 shader.
///
/// Emits the MilkDrop uniform prelude, HLSL compatibility macros, the body, and
/// a `main` entry point returning `vec4(ret, 1.0)`.
pub fn wrap_milk_shader(prelude: &str, body: &str, stage: MilkShaderStage) -> String {
    let _ = (prelude, body, stage);
    String::new()
}

/// Translate one MilkDrop shader stage into owned WGSL.
pub fn translate_milk_shader(
    prelude: &str,
    body: &str,
    stage: MilkShaderStage,
) -> Result<CompiledMilkShader, TranslateError> {
    let _ = prelude;
    let _ = body;
    Ok(CompiledMilkShader {
        wgsl: String::new(),
        source: String::new(),
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
        for (stage, body) in [(MilkShaderStage::Warp, WARP_BODY), (MilkShaderStage::Comp, COMP_BODY)]
        {
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
}
