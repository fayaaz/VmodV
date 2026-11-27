//! RED contract for `Cube/Evil Box.xml`. The scene is deliberately resolved by
//! registry name and its future source is read at runtime, so this test compiles
//! before `Plane9 Evil Box` or `scenes::evil_box` exists.

use std::fs;
use std::path::Path;

use crate::point10::audio_bridge::{AudioBridge, BandPayload, RESPONSIVENESS_PROPS};
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_DUAL_TEXTURE_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::test_support::{TestTarget, any_lit, gpu};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Evil Box";
const LEGACY_NAME: &str = "Point10 Evil Box";
const EVIL_BOX_LICENSE: &str = concat!("CC BY-", "NC-", "SA 4.0");

fn compact(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn production_source(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|source| {
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(&source)
            .to_string()
    })
}

fn raw_const(source: &str, name: &str) -> Option<String> {
    let declaration = source.find(&format!("const {name}"))?;
    let raw_start = declaration + source[declaration..].find("r#\"")? + 3;
    let raw_end = raw_start + source[raw_start..].find("\"#")?;
    Some(source[raw_start..raw_end].to_string())
}

fn texture_sample_level_calls(source: &str) -> Vec<&str> {
    const MARKER: &str = "textureSampleLevel(";
    let mut calls = Vec::new();
    for (start, _) in source.match_indices(MARKER) {
        let open = start + MARKER.len() - 1;
        let mut depth = 0usize;
        for (offset, character) in source[open..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        calls.push(&source[start..open + offset + 1]);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    calls
}

/// Validate a scene-owned WGSL literal with the same shared Plane9 preludes the
/// production scene composes. A literal may own its uniform/texture declarations
/// (Sound Cube style) or use the shared Plane9 declarations (Stargate style).
fn validates_with_plane9_preludes(shader: &str) -> bool {
    let common = format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n");
    let preludes = [
        String::new(),
        PLANE9_UNIFORMS_WGSL.to_string(),
        format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n"),
        format!("{PLANE9_UNIFORMS_WGSL}\n{PLANE9_DUAL_TEXTURE_WGSL}\n"),
    ];
    for prelude in preludes {
        let program = format!("{common}{prelude}{shader}");
        let Ok(module) = naga::front::wgsl::parse_str(&program) else {
            continue;
        };
        if naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .is_ok()
        {
            return true;
        }
    }
    false
}

#[test]
fn plane9_evil_box_fidelity_red_contract() {
    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($arg:tt)+) => {
            if !$condition {
                failures.push(format!($($arg)+));
            }
        };
    }

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenes = all_scenes();
    let names: Vec<&str> = scenes.iter().map(|scene| scene.name()).collect();
    let mut unique_names = names.clone();
    unique_names.sort_unstable();
    unique_names.dedup();

    // A — XML lines 2-6: one new canonical Plane9/Dahl scene, with the
    // historical alias accepted even though this preset never shipped under it.
    check!(
        scenes.len() == 50,
        "A: expected 50 Point10 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "A: duplicate registry names in {names:?}"
    );
    check!(
        PLANE9_SCENE_NAMES.len() == 32,
        "A: expected 32 Plane9 scene names, got {}",
        PLANE9_SCENE_NAMES.len()
    );
    let matches: Vec<_> = scenes
        .iter()
        .filter(|scene| scene.name() == SCENE_NAME)
        .collect();
    check!(
        matches.len() == 1,
        "A: {SCENE_NAME} must be registered exactly once, got {}",
        matches.len()
    );
    check!(
        PLANE9_SCENE_NAMES
            .iter()
            .filter(|name| **name == SCENE_NAME)
            .count()
            == 1,
        "A: {SCENE_NAME} must occur exactly once in PLANE9_SCENE_NAMES"
    );
    check!(
        canonical_scene_name(LEGACY_NAME) == Some(SCENE_NAME),
        "A: {LEGACY_NAME} must resolve to {SCENE_NAME}"
    );
    check!(
        resolve_scene_name(LEGACY_NAME) == SCENE_NAME,
        "A: resolve_scene_name must canonicalise {LEGACY_NAME}"
    );
    check!(
        gallery_kind(SCENE_NAME, "point10") == "plane9",
        "A: {SCENE_NAME} must have Gallery kind plane9"
    );
    if let Some(scene) = matches.first() {
        let meta = scene.meta();
        check!(
            meta.name == SCENE_NAME,
            "A: meta/name mismatch: {:?}",
            meta.name
        );
        check!(
            meta.author == PLANE9_AUTHOR,
            "A: author is {:?}",
            meta.author
        );
        check!(meta.version == "1.0.0", "A: version is {:?}", meta.version);
        check!(
            meta.module_type == "2d",
            "A: module type is {:?}",
            meta.module_type
        );
        check!(
            meta.audio_features == ["bass", "mid", "treble"],
            "A: audio features are {:?}",
            meta.audio_features
        );
    }

    // A/G — the parity fixture and generated catalogue are data contracts, not
    // implementation details. Both move from 49/500 to 50/501.
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("tests/fixtures/module-parity.json"))
            .expect("read module-parity.json"),
    )
    .expect("parse module-parity.json");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(50),
        "G: manifest point10 count is {:?}, expected 50",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(501),
        "G: manifest total is {:?}, expected 501",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 50,
        "G: manifest point10 array has {} names, expected 50",
        manifest_names.len()
    );
    check!(
        manifest_names
            .iter()
            .filter(|name| **name == SCENE_NAME)
            .count()
            == 1,
        "G: manifest must contain {SCENE_NAME} exactly once"
    );

    let catalogue: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
            .expect("read docs/modules/modules.json"),
    )
    .expect("parse docs/modules/modules.json");
    check!(
        catalogue["counts"]["point10"].as_u64() == Some(50),
        "G: catalogue point10 count is {:?}, expected 50",
        catalogue["counts"]["point10"]
    );
    check!(
        catalogue["counts"]["total"].as_u64() == Some(501),
        "G: catalogue total is {:?}, expected 501",
        catalogue["counts"]["total"]
    );
    let catalogue_matches: Vec<_> = catalogue["modules"]
        .as_array()
        .expect("catalogue modules array")
        .iter()
        .filter(|module| module["name"] == SCENE_NAME)
        .collect();
    check!(
        catalogue_matches.len() == 1,
        "G: catalogue must contain {SCENE_NAME} exactly once, got {}",
        catalogue_matches.len()
    );
    if let Some(module) = catalogue_matches.first() {
        check!(
            module["kind"] == "point10"
                && module["author"] == PLANE9_AUTHOR
                && module["version"] == "1.0.0"
                && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"]),
            "G: catalogue metadata is {:?}",
            module
        );
    }

    // A/G — XML line 6 states the exact 4.0 licence. The renderer crate cannot
    // call modv_ui::model::module_license, so pin its arm and notices row exactly
    // as the sibling fidelity contracts do.
    let notices = fs::read_to_string(repo_root.join("THIRD_PARTY_NOTICES.md"))
        .expect("read THIRD_PARTY_NOTICES.md");
    let notice_rows: Vec<_> = notices
        .lines()
        .filter(|line| line.starts_with("| Evil Box |"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if row.contains("evil_box.rs")
                && row.contains(EVIL_BOX_LICENSE)
                && row.contains("Joakim Dahl") => {}
        [row] => check!(false, "G: wrong Evil Box notices row {row:?}"),
        [] => check!(false, "G: missing Evil Box notices row"),
        _ => check!(false, "G: duplicate Evil Box notices rows {notice_rows:?}"),
    }
    let ui_model = fs::read_to_string(repo_root.join("crates/modv-ui/src/model.rs"))
        .expect("read modv-ui model.rs");
    let v4_arm = ui_model
        .split("// Plane9 ports whose own headers state 4.0.")
        .nth(1)
        .and_then(|arm| arm.split("// Point10 variants").next())
        .unwrap_or_default();
    check!(
        v4_arm.contains("\"Evil Box\""),
        "G: module_license exact-4.0 arm is missing Evil Box"
    );
    check!(
        compact(&ui_model).contains("checked.len(),50,"),
        "G: UI licence provenance count must move from 49 to 50"
    );

    // A — the app wrapper must construct the same canonical/legacy names and
    // delegate every Point10Scene method plus apply_props.
    let app_registry = production_source(&repo_root.join("crates/modv-app/src/module_registry.rs"))
        .expect("read app module registry");
    let compact_app = compact(&app_registry);
    check!(
        compact_app.contains("EvilBox(EvilBox)"),
        "A: Point10SceneKind is missing EvilBox(EvilBox)"
    );
    check!(
        app_registry.contains("\"Plane9 Evil Box\" => Self::EvilBox(EvilBox::new())"),
        "A: Point10SceneKind::create is missing Plane9 Evil Box"
    );
    check!(
        compact_app.matches("Self::EvilBox(scene)").count() == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_app.matches("Self::EvilBox(scene)").count()
    );

    // F — sceneProps.js has no Evil Box entry (SCENE_PROP_CONFIGS lines 282-426
    // in the 1.x tree), so the schema is exactly responsiveness + Lens.
    let expected_props = plane9_scene_props(&[]);
    let expected_prop_names: Vec<&str> = expected_props.iter().map(|prop| prop.name).collect();
    check!(
        expected_prop_names
            == [
                "gain",
                "smoothing",
                "bassGain",
                "midGain",
                "trebleGain",
                "useAgc",
                "lens",
            ],
        "F: Plane9 no-scene-prop schema changed: {expected_prop_names:?}"
    );
    check!(
        expected_props.len() == RESPONSIVENESS_PROPS.len() + 1
            && expected_props[RESPONSIVENESS_PROPS.len()] == LENS_PROP,
        "F: Lens must follow the responsiveness levers"
    );
    let projection = perspective(45.0_f32.to_radians(), 1280.0 / 720.0, 0.1, 1000.0);
    check!(
        apply_lens(projection, lens_value(Some(1.0))) == projection,
        "F: Lens 1.0 must preserve the projection bit-for-bit"
    );
    let doubled = apply_lens(projection, lens_value(Some(2.0)));
    check!(
        doubled[0][0] == projection[0][0] * 2.0
            && doubled[1][1] == projection[1][1] * 2.0
            && doubled[0][1..] == projection[0][1..]
            && doubled[1][0] == projection[1][0]
            && doubled[1][2..] == projection[1][2..]
            && doubled[2] == projection[2]
            && doubled[3] == projection[3],
        "F: Lens must scale only projection x/y"
    );
    check!(
        lens_value(None) == 1.0
            && lens_value(Some(f32::NAN)) == 1.0
            && lens_value(Some(0.01)) == 0.25
            && lens_value(Some(99.0)) == 4.0,
        "F: Lens defaults/clamps changed"
    );

    let evil_box_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = evil_box_index {
        let scene = &scenes[index];
        check!(
            scene.props() == expected_props,
            "F: Evil Box props must be responsiveness + Lens only; got {:?}",
            scene.props()
        );
    }

    // B-E — read the not-yet-existing module at runtime. All constants and
    // shader contracts below cite the XML line(s) they transcribe.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/evil_box.rs");
    let source = production_source(&scene_path);
    check!(
        source.is_some(),
        "B-E: missing scene source {}",
        scene_path.display()
    );
    if let Some(source) = source {
        let compact_source = compact(&source);

        // A — XML lines 3-6: provenance stays visible in the port's source.
        for needle in [
            "Cube/Evil Box.xml",
            "Joakim Dahl",
            EVIL_BOX_LICENSE,
            "Light trying to escape an evil box",
            "cube,vr",
        ] {
            check!(source.contains(needle), "A: scene source lost {needle:?}");
        }

        // B — Screen1 (XML 10, 12, 14-17), RenderObject1 (21-23), Cube1
        // (85-91), and the Vector1 seed (93-97).
        for needle in [
            "CAMERA_POSITION:[f32;3]=[0.0,0.0,3.38];",
            "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,-1.0];",
            "CAMERA_FOV:f32=45.0;",
            "CAMERA_NEAR:f32=0.1;",
            "CAMERA_FAR:f32=1000.0;",
            "SCALE_BY_ASPECT:bool=true;",
            "MODEL_SCALE:[f32;3]=[1.01251,1.01251,1.0];",
            "INITIAL_ROTATION:[f32;3]=[451.89,-421.757,-236.581];",
            "CUBE_SIZE:[f32;3]=[1.0,1.0,1.0];",
            "build_box(1.0,1.0,1.0)",
            "permrand:1.0",
        ] {
            check!(compact_source.contains(needle), "B: lost constant {needle}");
        }
        check!(
            compact_source.contains("perspective(")
                && compact_source.contains("CAMERA_FOV.to_radians()")
                && compact_source.contains("apply_lens("),
            "B: camera must use vertical-FOV perspective and apply Lens"
        );
        check!(
            compact_source.contains("look_at(")
                && compact_source.contains("CAMERA_POSITION[2]+CAMERA_LOOK_AT[2]"),
            "B: relative CamLookAt must be added to CamPos before look_at"
        );
        check!(
            compact_source.contains("euler_xyz(")
                && compact_source.contains(".to_radians()")
                && compact_source.contains("MODEL_SCALE"),
            "B: model must compose XYZ Euler radians with MODEL_SCALE"
        );

        // B — Expression1 (XML 283-295), connected to Vector1/RenderObject.Rot
        // at 309 and 312-314. Keep the vendor formula visible and the three
        // noise2 axes in the Rust drive.
        for expression in [
            "perm = perm + ((min(0.03, band(-1, 0, 0, 0.03)*0.3+0.001))*deltatime);",
            "t = perm + permrand*10;",
            "out1=noise2(0,t)*720;",
            "out2=noise2(1,t)*720;",
            "out3=noise2(2,t)*720;",
        ] {
            check!(
                compact_source.contains(&compact(expression)),
                "B: Expression1 lost {expression}"
            );
        }
        check!(
            compact_source.contains("band(-1,0,0,0.03)")
                && compact_source.contains("*0.3+0.001")
                && compact_source.contains(".min(0.03)")
                && compact_source.contains("permrand*10.0")
                && compact_source.matches("value_noise2(").count() >= 3
                && compact_source.matches("*720.0").count() >= 3,
            "B: Rust drive must implement Expression1's damping-0 bass phase and three 720-degree noise axes"
        );
        for coordinate in ["0.0", "1.0", "2.0"] {
            check!(
                compact_source.contains(&format!("value_noise2(t,{coordinate})")),
                "B: Rust drive is missing noise2 axis {coordinate}"
            );
        }

        // Shader1 expression (XML 80-82): beat(0) drives x, the noise-selected
        // fallback drives y. Both remain part of the surface shader contract.
        for expression in [
            "in1.x = min(0.4,beat(0)*10);",
            "in1.y = 0.1+beat((noise1(time*5)*0.5+0.5)*0.1)*10;",
        ] {
            check!(
                compact_source.contains(&compact(expression)),
                "D: Shader1 expression lost {expression}"
            );
        }
        check!(
            compact_source.contains("beat(0)")
                && compact_source.contains(".min(0.4)")
                && compact_source.contains("noise1(time*5.0)")
                && compact_source.contains("0.1+"),
            "D: Rust surface drive must implement both Shader1 expression lanes"
        );

        // C — RenderToTexture1 (XML 149-158): Format 5 is the vendor's
        // four-channel half-float target here; Width/Height 0 means 100%, not
        // WidthCustom/HeightCustom 256. CreateMipMaps is recorded but inert:
        // every capture sample is 1:1 at LOD 0, so the port follows the other
        // Plane9 scenes' single-mip convention instead of generating unused mips.
        for needle in [
            "RTT_FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba16Float;",
            "RTT_MODE:u32=0;",
            "RTT_MIPS:bool=true;",
            "mip_level_count:1",
        ] {
            check!(
                compact_source.contains(needle),
                "C: RTT contract lost {needle}"
            );
        }
        let lower_source = source.to_ascii_lowercase();
        check!(
            lower_source.contains("rtt_mips")
                && lower_source.contains("recorded")
                && lower_source.contains("inert")
                && lower_source.contains("single-mip")
                && lower_source.contains("lod 0"),
            "C: RTT_MIPS must be documented as recorded from XML but inert under the single-mip/LOD-0 convention"
        );
        check!(
            !compact_source.contains("rtt_mip_levels")
                && !compact_source.contains("MipTarget")
                && !compact_source.contains("fs_mip")
                && !compact_source.contains("mip_pipeline")
                && !compact_source.contains("mip_binds")
                && !compact_source.contains("base_mip_level"),
            "C: single-mip capture must not retain a mip helper, MipTarget, generation pass, or per-level views"
        );
        let level_samples = texture_sample_level_calls(&source);
        check!(
            !level_samples.is_empty(),
            "C: capture shaders must use explicit level-0 textureSampleLevel calls"
        );
        for sample in level_samples {
            check!(
                compact(sample).ends_with(",0.0)"),
                "C: capture sample must use LOD 0: {sample}"
            );
        }
        check!(
            !compact_source.contains("RTT_SIZE:u32=256")
                && !compact_source.contains("width:256")
                && !compact_source.contains("height:256"),
            "C: full-scene RTT must not use the vendor's inactive 256x256 custom size"
        );

        // C — Blur1 (243-247), CopyTexture1 (248-282), StoreTexture1
        // (298-300), PreviousLayer1 (301-303), and graph edges 317-325.
        for needle in [
            "BLUR_DIRECTION:u32=2;",
            "BLUR_WIDTH:f32=4.0;",
            "BLUR_BRIGHTNESS:f32=1.0;",
            "STORE_SWAP_DELAY:f32=0.0;",
            "COPY_MIPS:bool=false;",
            "PREVIOUS_LAYER_MIPS:bool=false;",
            "feedback_current=1-self.feedback_current",
        ] {
            check!(
                compact_source.contains(needle),
                "C: feedback contract lost {needle}"
            );
        }
        check!(
            compact_source.contains("StoreTexture1")
                && compact_source.contains("CopyTexture1")
                && compact_source.contains("PreviousLayer1")
                && compact_source.contains("Blur1"),
            "C: feedback graph node provenance is missing"
        );
        check!(
            compact_source.contains("textureSampleLevel(g_texture1,g_sampler,tex,0.0).rgb,0.0")
                || compact_source.contains("textureSample(g_texture1,g_sampler,tex).rgb,0.0"),
            "C: CopyTexture must copy RGB and force alpha zero (XML 277)"
        );

        // D — Shader1 surface (XML 43-76). _noisefast aliases _noise in
        // P9Glsl.js:64, so the existing p9_noise helper is the correct WGSL call.
        let cube = raw_const(&source, "EVIL_BOX_CUBE_WGSL");
        check!(cube.is_some(), "D: missing EVIL_BOX_CUBE_WGSL literal");
        if let Some(cube) = cube {
            let compact_cube = compact(&cube);
            for needle in [
                "u.view_proj",
                "u.model",
                "in.tex*2.0-1.0",
                "p9_turbulence(vec3<f32>(p*10.0,u.time),3u)",
                "p9_smoothstep(0.2,1.0,",
                "*u.in1.y",
                "p9_noise(vec3<f32>(p,u.time*2.0))*0.5+0.4",
                "vec3<f32>(v*2.0)",
                "vec4<f32>(vec3<f32>(v*2.0),1.0)",
            ] {
                check!(
                    compact_cube.contains(needle),
                    "D: cube shader lost {needle}"
                );
            }
            check!(
                validates_with_plane9_preludes(&cube),
                "D: cube WGSL does not parse/validate with the Plane9 helpers"
            );
        }

        // D/C — Shader3 + CopyTexture + Blur1 (XML 199-282): additive blurred
        // history, 0.95 zoom, 0.85 gain, alpha-zero copy and width-4 blur.
        let pass = raw_const(&source, "EVIL_BOX_PASS_WGSL");
        check!(pass.is_some(), "D: missing EVIL_BOX_PASS_WGSL literal");
        if let Some(pass) = pass {
            let compact_pass = compact(&pass);
            for needle in [
                "(p*0.95)*0.5+0.5",
                "*0.85",
                "textureSampleLevel(g_texture1,g_sampler,tex,0.0).rgb,0.0",
                "u.in2.x*u.in2.x",
                "*u.in2.y",
            ] {
                check!(
                    compact_pass.contains(needle),
                    "D: feedback shader lost {needle}"
                );
            }
            check!(
                validates_with_plane9_preludes(&pass),
                "D: copy/blur/zoom WGSL does not parse/validate with the Plane9 helpers"
            );
        }

        // D — Shader2 composite (XML 116-140): Texture1 is the cube RTT,
        // Texture2 is PreviousLayer, and the distinctive mix-to-black + additive
        // source expression is retained verbatim in WGSL.
        let composite = raw_const(&source, "EVIL_BOX_COMPOSITE_WGSL");
        check!(
            composite.is_some(),
            "D: missing EVIL_BOX_COMPOSITE_WGSL literal"
        );
        if let Some(ref composite_shader) = composite {
            let compact_composite = compact(composite_shader);
            for needle in [
                "textureSampleLevel(g_texture1,g_sampler,tex,0.0)",
                "textureSampleLevel(g_texture2,g_sampler,tex,0.0)",
                "*diffuse",
                "mix(bg,vec4<f32>(0.0),c.a)+c",
            ] {
                check!(
                    compact_composite.contains(needle),
                    "D: composite shader lost {needle}"
                );
            }
            check!(
                validates_with_plane9_preludes(composite_shader),
                "D: composite WGSL does not parse/validate with the Plane9 helpers"
            );
        }
        check!(
            compact_source.contains("PLANE9_DUAL_TEXTURE_WGSL")
                || composite
                    .as_ref()
                    .is_some_and(|shader| shader.contains("g_texture2")),
            "D: composite must bind both cube capture and PreviousLayer"
        );

        // The RTT's inline fbm shader (XML 162-190) is dead: RenderRect2.Render
        // is connected to RenderObject1.Render (319-320), so the cube chain
        // overrides it, exactly as in Sound Cube.
        check!(
            !compact_source.contains("p9_fbm(") && !compact_source.contains("_fbm("),
            "D: dead RenderToTexture1 inline fbm pass must not be implemented"
        );
    }

    // E — 1280x720 fallback-adapter smoke. dt=0 and silent audio freeze time,
    // Expression1 and Shader1's inputs, so frame 2 can only change through the
    // StoreTexture/CopyTexture/Blur feedback state. A second fresh scene must
    // reproduce frame 1 exactly.
    if let Some(index) = evil_box_index {
        if let Some((device, queue)) = gpu() {
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Evil Box registry lookup");
            let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            ctx.resize(1280, 720);
            scene.init(&mut ctx);
            scene.resize(1280, 720);
            let mut repeat_ctx =
                SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            repeat_ctx.resize(1280, 720);
            repeat.init(&mut repeat_ctx);
            repeat.resize(1280, 720);

            let payload = BandPayload::silent(44_100.0);
            let mut bridge = AudioBridge::new();
            let mut repeat_bridge = AudioBridge::new();
            let target = TestTarget::new(&device, 1280, 720);
            let repeat_target = TestTarget::new(&device, 1280, 720);
            let render = |scene: &mut Box<dyn Point10Scene>,
                          bridge: &mut AudioBridge,
                          target: &TestTarget| {
                bridge.update(0.0, Some(&payload), &Default::default());
                scene.update(0.0, bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-evil-box-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                target.read(&device, &queue)
            };

            let first = render(&mut scene, &mut bridge, &target);
            let repeated = render(&mut repeat, &mut repeat_bridge, &repeat_target);
            let second = render(&mut scene, &mut bridge, &target);
            check!(any_lit(&first), "E: Evil Box frame 1 is empty at 1280x720");
            check!(any_lit(&second), "E: Evil Box frame 2 is empty at 1280x720");
            check!(
                first == repeated,
                "E: Evil Box frame 1 is not deterministic across fresh instances"
            );
            check!(
                first != second,
                "E: frozen-time Evil Box feedback did not evolve between frames"
            );
            scene.dispose();
            repeat.dispose();
        } else {
            eprintln!("skipping Plane9 Evil Box GPU smoke: no fallback adapter");
        }
    }

    assert!(
        failures.is_empty(),
        "Plane9 Evil Box RED contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
