//! RED contract for `Line/Waves of sin.xml`. The scene is resolved through the
//! registry and its future source is read at runtime, so this compiles before
//! `Plane9 Waves of Sin`, `WavesOfSin`, or either scene-owned shader exists.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::point10::audio_bridge::{AudioBridge, BandPayload, RESPONSIVENESS_PROPS};
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_TONEMAP_WGSL, PLANE9_UNIFORMS_WGSL,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::test_support::{TestTarget, compact_contains, compact_match_count, gpu};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Waves of Sin";
const LEGACY_NAME: &str = "Point10 Waves of Sin";
const WAVES_OF_SIN_LICENSE: &str = concat!("CC BY-", "NC-", "SA 4.0");
const NC_SA_MARKER: &str = concat!("CC BY-", "NC-", "SA");

fn production_source(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|source| {
        source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(&source)
            .to_string()
    })
}

/// Remove Rust line comments for negative implementation checks. Provenance
/// comments remain in `source` and are checked separately.
fn rust_code(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn raw_const(source: &str, name: &str) -> Option<String> {
    let declaration = source.find(&format!("const {name}"))?;
    let raw_start = declaration + source[declaration..].find("r#\"")? + 3;
    let raw_end = raw_start + source[raw_start..].find("\"#")?;
    Some(source[raw_start..raw_end].to_string())
}

/// Slice one function from `header` through its matching closing brace.
fn fn_scope<'a>(source: &'a str, header: &str) -> Option<&'a str> {
    let start = source.find(header)?;
    let open = start + source[start..].find('{')?;
    let mut depth = 0usize;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[start..open + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let scope = fn_scope(source, &format!("fn {name}"))?;
    let open = scope.find('{')? + 1;
    Some(&scope[open..scope.len() - 1])
}

/// Keep the header in the slice, including `(`, so a whole-shader needle cannot
/// be satisfied by a sibling helper's body.
fn shader_fn_scope<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    fn_scope(source, &format!("fn {name}("))
}

/// Validate a scene-owned literal with the shared Plane9 preludes. The first
/// combination covers a self-contained point shader; the later combinations
/// cover a composite that uses the shared tone-map/batch/uniform declarations.
fn validates_with_plane9_preludes(shader: &str) -> bool {
    let preludes = [
        format!("{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}"),
        format!("{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PLANE9_BATCH_WGSL}"),
        format!(
            "{P9_COMMON_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_UNIFORMS_WGSL}"
        ),
    ];
    for prelude in preludes {
        let program = format!("{prelude}\n{shader}");
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

fn rgb_lit_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0)
        .count()
}

fn all_alpha_zero(pixels: &[u8]) -> bool {
    pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0)
}

#[test]
fn plane9_waves_of_sin_fidelity_red_contract() {
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
    let repeat_names: Vec<&str> = all_scenes().iter().map(|scene| scene.name()).collect();
    let mut unique_names = names.clone();
    unique_names.sort_unstable();
    unique_names.dedup();

    // A — XML 2-6: the ninth XML batch appends one canonical Plane9/Dahl scene
    // after Cell Culture. The XML Desc is one space, so no description is invented.
    check!(
        scenes.len() == 54,
        "A: expected 54 Point10 scenes, got {}",
        scenes.len()
    );
    check!(
        unique_names.len() == names.len(),
        "A: duplicate registry names in {names:?}"
    );
    check!(
        names == repeat_names,
        "A: all_scenes roster order is not deterministic"
    );
    check!(
        names.last() == Some(&"Plane9 Gravity Spectrum"),
        "A: Gravity Spectrum must be the deterministic roster tail, got {:?}",
        names.last()
    );
    check!(
        names.get(names.len().saturating_sub(2)) == Some(&SCENE_NAME),
        "A: Waves of Sin must follow Plane9 Cell Culture without reordering the roster"
    );
    check!(
        names.get(names.len().saturating_sub(3)) == Some(&"Plane9 Cell Culture"),
        "A: Waves of Sin must follow Plane9 Cell Culture without reordering the roster"
    );
    check!(
        PLANE9_SCENE_NAMES.len() == 36,
        "A: expected 36 Plane9 scene names, got {}",
        PLANE9_SCENE_NAMES.len()
    );
    let mut unique_plane9_names = PLANE9_SCENE_NAMES.to_vec();
    unique_plane9_names.sort_unstable();
    unique_plane9_names.dedup();
    check!(
        unique_plane9_names.len() == PLANE9_SCENE_NAMES.len(),
        "A: duplicate names in PLANE9_SCENE_NAMES"
    );
    check!(
        PLANE9_SCENE_NAMES.last() == Some(&"Plane9 Gravity Spectrum"),
        "A: PLANE9_SCENE_NAMES must end with Plane9 Gravity Spectrum"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(2)) == Some(&SCENE_NAME),
        "A: PLANE9_SCENE_NAMES must append Waves of Sin after Cell Culture"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(3))
            == Some(&"Plane9 Cell Culture"),
        "A: PLANE9_SCENE_NAMES must append Waves of Sin after Cell Culture"
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
        check!(meta.name == SCENE_NAME, "A: meta/name is {:?}", meta.name);
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

    // A — fixture/catalogue counts move from 52/503 to 54/505. XML line 6 is
    // the exact 4.0 licence arm, not the 3.0 arm.
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("tests/fixtures/module-parity.json"))
            .expect("read module-parity.json"),
    )
    .expect("parse module-parity.json");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(54),
        "A: manifest point10 count is {:?}, expected 54",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(505),
        "A: manifest total is {:?}, expected 505",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 54,
        "A: manifest point10 array has {} names, expected 54",
        manifest_names.len()
    );
    check!(
        manifest_names
            .iter()
            .filter(|name| **name == SCENE_NAME)
            .count()
            == 1,
        "A: manifest must contain {SCENE_NAME} exactly once"
    );
    check!(
        manifest_names.last() == Some(&"Plane9 Gravity Spectrum")
            && manifest_names.get(manifest_names.len().saturating_sub(2)) == Some(&SCENE_NAME)
            && manifest_names.get(manifest_names.len().saturating_sub(3))
                == Some(&"Plane9 Cell Culture"),
        "A: manifest must append {SCENE_NAME} after Plane9 Cell Culture"
    );

    let catalogue: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
            .expect("read docs/modules/modules.json"),
    )
    .expect("parse docs/modules/modules.json");
    check!(
        catalogue["counts"]["point10"].as_u64() == Some(54),
        "A: catalogue point10 count is {:?}, expected 54",
        catalogue["counts"]["point10"]
    );
    check!(
        catalogue["counts"]["total"].as_u64() == Some(505),
        "A: catalogue total is {:?}, expected 505",
        catalogue["counts"]["total"]
    );
    check!(
        catalogue["modules"].as_array().map(Vec::len) == Some(505),
        "A: catalogue modules array must contain 505 entries"
    );
    let catalogue_matches: Vec<_> = catalogue["modules"]
        .as_array()
        .expect("catalogue modules array")
        .iter()
        .filter(|module| module["name"] == SCENE_NAME)
        .collect();
    check!(
        catalogue_matches.len() == 1,
        "A: catalogue must contain {SCENE_NAME} exactly once, got {}",
        catalogue_matches.len()
    );
    if let Some(module) = catalogue_matches.first() {
        check!(
            module["kind"] == "point10"
                && module["author"] == PLANE9_AUTHOR
                && module["version"] == "1.0.0"
                && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"]),
            "A: catalogue metadata is {:?}",
            module
        );
        check!(
            module["description"].is_null(),
            "A: the single-space XML Desc must use the sibling null-description convention, got {:?}",
            module["description"]
        );
        let prop_names: Vec<&str> = module["props"]
            .as_array()
            .expect("catalogue Waves of Sin props")
            .iter()
            .map(|prop| prop["name"].as_str().expect("catalogue prop name"))
            .collect();
        check!(
            prop_names
                == [
                    "gain",
                    "smoothing",
                    "bassGain",
                    "midGain",
                    "trebleGain",
                    "useAgc",
                    "lens",
                ],
            "A: catalogue props are {prop_names:?}"
        );
    }

    let notices = fs::read_to_string(repo_root.join("THIRD_PARTY_NOTICES.md"))
        .expect("read THIRD_PARTY_NOTICES.md");
    let notice_rows: Vec<_> = notices
        .lines()
        .filter(|line| line.starts_with('|') && compact_contains(line, "waves_of_sin.rs"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if compact_contains(row, WAVES_OF_SIN_LICENSE)
                && compact_contains(row, "Joakim Dahl") => {}
        [row] => check!(false, "A: wrong Waves of Sin notices row {row:?}"),
        [] => check!(false, "A: missing Waves of Sin notices row"),
        _ => check!(
            false,
            "A: duplicate Waves of Sin notices rows {notice_rows:?}"
        ),
    }
    let ui_model = fs::read_to_string(repo_root.join("crates/modv-ui/src/model.rs"))
        .expect("read modv-ui model.rs");
    let v4_arm = ui_model
        .split("// Plane9 ports whose own headers state 4.0.")
        .nth(1)
        .and_then(|arm| arm.split("// Point10 variants").next())
        .unwrap_or_default();
    let v3_arm = ui_model
        .split("// Plane9 ports whose own headers state 3.0")
        .nth(1)
        .and_then(|arm| arm.split("// Plane9 shader effects").next())
        .unwrap_or_default();
    check!(
        compact_contains(v4_arm, "\"Waves of Sin\""),
        "A: module_license exact-4.0 arm is missing Waves of Sin"
    );
    check!(
        !compact_contains(v3_arm, "\"Waves of Sin\""),
        "A: Waves of Sin must not be classified in the 3.0 licence arm"
    );
    check!(
        compact_contains(&ui_model, "\"Point10 Waves of Sin\""),
        "A: UI provenance test is missing Point10 Waves of Sin"
    );
    check!(
        compact_contains(&ui_model, "checked.len(),54,"),
        "A: UI licence provenance count must move from 53 to 54"
    );

    // A — the app wrapper must construct the canonical name and delegate every
    // Point10Scene method plus apply_props, without naming a future Rust type.
    let app_registry = production_source(&repo_root.join("crates/modv-app/src/module_registry.rs"))
        .expect("read app module registry");
    check!(
        compact_contains(
            &app_registry,
            "usemodv_renderers::point10::scenes::waves_of_sin::WavesOfSin;"
        ),
        "A: app registry is missing the WavesOfSin import"
    );
    check!(
        compact_contains(&app_registry, "WavesOfSin(WavesOfSin)"),
        "A: Point10SceneKind is missing WavesOfSin(WavesOfSin)"
    );
    check!(
        compact_contains(
            &app_registry,
            "\"Plane9 Waves of Sin\"=>Self::WavesOfSin(WavesOfSin::new())"
        ),
        "A: Point10SceneKind::create is missing Plane9 Waves of Sin"
    );
    check!(
        compact_match_count(&app_registry, "Self::WavesOfSin(scene)") == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_match_count(&app_registry, "Self::WavesOfSin(scene)")
    );
    let app_registry_with_tests =
        fs::read_to_string(repo_root.join("crates/modv-app/src/module_registry.rs"))
            .expect("read app module registry with tests");
    let wrapper_order = fn_body(
        &app_registry_with_tests,
        "point10_definitions_match_the_wrapper_modules",
    )
    .unwrap_or_default();
    let cell_position = wrapper_order.find("\"Plane9 Cell Culture\"");
    let waves_position = wrapper_order.find("\"Plane9 Waves of Sin\"");
    let gravity_position = wrapper_order.find("\"Plane9 Gravity Spectrum\"");
    check!(
        cell_position.is_some_and(|cell| {
            waves_position.is_some_and(|waves| {
                gravity_position.is_some_and(|gravity| cell < waves && waves < gravity)
            })
        }),
        "A: wrapper-order list must append Plane9 Gravity Spectrum after Plane9 Waves of Sin"
    );
    if let Some(gravity) = gravity_position {
        let tail = &wrapper_order[gravity + "\"Plane9 Gravity Spectrum\"".len()..];
        let tail = tail.trim_start();
        let tail = tail.strip_prefix(',').unwrap_or(tail).trim_start();
        check!(
            tail.starts_with(']'),
            "A: Plane9 Gravity Spectrum must be the wrapper-order list tail"
        );
    }

    // A/props — no vendor sceneProps.js exists and P9Nodes/P9Scene define no
    // Waves of Sin override, so the schema is exactly responsiveness + Lens.
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
        "A: Plane9 no-scene-prop schema changed: {expected_prop_names:?}"
    );
    check!(
        expected_props.len() == RESPONSIVENESS_PROPS.len() + 1
            && expected_props[RESPONSIVENESS_PROPS.len()] == LENS_PROP,
        "A: Lens must follow the responsiveness levers"
    );
    check!(
        !compact_contains(
            production_source(
                &repo_root.join("crates/modv-renderers/src/point10/scenes/waves_of_sin.rs")
            )
            .as_deref()
            .unwrap_or_default(),
            "sceneProps.js"
        ),
        "A: no vendor sceneProps.js exists; the port must not cite a nonexistent override file"
    );
    let projection = perspective(45.0_f32.to_radians(), 1280.0 / 720.0, 0.1, 100.0);
    check!(
        apply_lens(projection, lens_value(Some(1.0))) == projection,
        "A: Lens 1.0 must preserve the projection bit-for-bit"
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
        "A: Lens must scale only projection x/y"
    );
    check!(
        lens_value(None) == 1.0
            && lens_value(Some(f32::NAN)) == 1.0
            && lens_value(Some(0.01)) == 0.25
            && lens_value(Some(99.0)) == 4.0,
        "A: Lens defaults/clamps changed"
    );
    let waves_of_sin_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = waves_of_sin_index {
        check!(
            scenes[index].props() == expected_props,
            "A: Waves of Sin props must be responsiveness + Lens only; got {:?}",
            scenes[index].props()
        );
    }

    // B-G — read the not-yet-existing module at runtime. All constants and
    // shader contracts below cite the XML/JS lines they transcribe.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/waves_of_sin.rs");
    let source = production_source(&scene_path);
    if source.is_none() {
        for group in ["A", "B", "C", "D", "E", "F", "G"] {
            failures.push(format!(
                "{group}: missing scene source {}",
                scene_path.display()
            ));
        }
        check!(
            shader_fn_scope(P9_COMMON_WGSL, "p9_bump").is_some(),
            "E/p9_bump: missing shared function scope"
        );
        check!(
            shader_fn_scope(P9_COMMON_WGSL, "p9_lift_gamma_gain").is_some(),
            "E/p9_lift_gamma_gain: missing shared function scope"
        );
    }
    if let Some(source) = source.as_deref() {
        let code = rust_code(source);
        let header = source
            .split("#![forbid(unsafe_code)]")
            .next()
            .unwrap_or(source);

        // A — XML 3-6: exact provenance and licence; Desc stays null.
        for needle in ["Waves of sin.xml", "Joakim Dahl", "Tags: `mellow,party`"] {
            check!(
                compact_contains(source, needle),
                "A: scene source lost {needle:?}"
            );
        }
        check!(
            compact_contains(source, WAVES_OF_SIN_LICENSE),
            "A: scene source must state the exact 4.0 licence"
        );
        let licence_declaration = source
            .lines()
            .find(|line| compact_contains(line, "const WAVES_OF_SIN_LICENSE"))
            .unwrap_or_default();
        check!(
            compact_contains(licence_declaration, "concat!("),
            "A: the licence declaration must use the split concat! form"
        );
        check!(
            !compact_contains(licence_declaration, NC_SA_MARKER),
            "A: the licence declaration must not contain contiguous NC-SA scanner text"
        );
        check!(
            !compact_contains(header, "description is")
                && !compact_contains(header, "own description"),
            "A: the single-space XML Desc must not acquire an invented description"
        );

        // B — Screen1 (XML 8-17), RenderRects (22-30), Clear1 (19-21), and
        // Clear2 (123-125). P9Scene.js:560-580 makes CamLookAt relative and
        // applies CamRot after lookAt as local-space rotateX/rotateY/rotateZ.
        // P9Scene.js:569 uses the active target aspect; ScaleByAspect is recorded
        // from the XML but is not read by the vendor JS implementation.
        for needle in [
            "CAMERA_POSITION:[f32;3]=[0.0,0.0,0.7];",
            "CAMERA_ROTATION:[f32;3]=[0.0,0.0,0.0];",
            "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,-1.0];",
            "CAMERA_LOOK_AT_IN_WORLD_SPACE:bool=false;",
            "CAMERA_FOV:f32=75.0;",
            "CAMERA_NEAR:f32=0.1;",
            "CAMERA_FAR:f32=100.0;",
            "SCALE_BY_ASPECT:bool=false;",
            "CLEAR1:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:0.0};",
            "CLEAR2:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:0.0};",
        ] {
            check!(
                compact_contains(source, needle),
                "B: lost camera/clear constant {needle}"
            );
        }
        check!(
            compact_contains(source, "relative")
                && compact_contains(source, "added to the position"),
            "B: CamLookAt (0,0,-1) must be documented as relative to CamPos"
        );
        check!(
            compact_contains(source, "plane9_scene_props(&[])")
                && compact_contains(&code, "lens_value(")
                && compact_contains(&code, "apply_lens("),
            "A/B: Waves of Sin must use the standard no-override props and Lens helpers"
        );
        if let Some(body) = fn_body(&code, "view_proj") {
            let look = body.find("look_at(");
            let rotate = body.find("euler_xyz(");
            check!(
                look.is_some_and(|look| rotate.is_some_and(|rotate| look < rotate)),
                "B: CamRot must be applied after lookAt, not discarded by a plain look_at"
            );
            for needle in [
                "CAMERA_POSITION[0]+CAMERA_LOOK_AT[0]",
                "CAMERA_POSITION[1]+CAMERA_LOOK_AT[1]",
                "CAMERA_POSITION[2]+CAMERA_LOOK_AT[2]",
                "&look_at(",
                "&euler_xyz(",
                "CAMERA_ROTATION[0].to_radians()",
                "CAMERA_ROTATION[1].to_radians()",
                "CAMERA_ROTATION[2].to_radians()",
                "apply_lens(perspective(",
                "CAMERA_FOV.to_radians()",
                "self.camera_aspect",
            ] {
                check!(compact_contains(body, needle), "B: view_proj lost {needle}");
            }
        } else {
            check!(false, "B: missing view_proj implementation");
        }

        // C — XML 126-128 and 260-270: 50,000 procedural points, the reusable
        // Width 2 / History 50 SoundTexture, a full-size single-mip RTT, and a
        // transparent-black PreviousLayer placeholder. The RTT inline shader is
        // dead because RenderObject1.Render feeds RenderToTexture1.Render.
        for needle in [
            "TOTAL_POINTS:u32=50000;",
            "POINTS_PER_WAVE:u32=1000;",
            "WAVE_COUNT:u32=50;",
            "RTT_FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba16Float;",
            "RTT_MODE:u32=0;",
            "RTT_MIPS:bool=false;",
            "PREVIOUS_LAYER_MIPS:bool=false;",
            "mip_level_count:1",
            "SOUND_TEXTURE_SOUND_TYPE:u32=0;",
            "SOUND_TEXTURE_WIDTH:u32=2;",
            "SOUND_TEXTURE_HISTORY:u32=50;",
            "SOUND_TEXTURE_DAMPING:f32=0.1;",
            "SOUND_TEXTURE_INCREASE_RATE:f32=10.0;",
            "SOUND_TEXTURE_DECREASE_RATE:f32=0.5;",
            "SOUND_TEXTURE_AMPLIFICATION:f32=5.0;",
            "SoundTextureConfig",
            "SoundTexture::new",
        ] {
            check!(
                compact_contains(source, needle),
                "C: lost graph/SoundTexture/RTT term {needle}"
            );
        }
        for needle in [
            "Clear2",
            "RenderRect2",
            "Shader1",
            "RenderToTexture1",
            "Bloom1",
            "Shader2",
            "RenderRect1",
            "Screen1",
            "SoundTexture1",
            "PreviousLayer1",
            "100%",
            "single-mip",
        ] {
            check!(
                compact_contains(source, needle),
                "C: graph provenance lost {needle}"
            );
        }
        check!(
            compact_contains(source, "PreviousLayer1")
                && compact_contains(source, "placeholder")
                && compact_contains(source, "transparent black"),
            "C: PreviousLayer1 must keep a transparent-black standalone placeholder"
        );
        check!(
            !compact_contains(&code, "StoreTexture"),
            "C: this graph has no StoreTexture feedback node"
        );
        for forbidden in [
            "p9_fbm(",
            "_fbm(",
            "RTT_SIZE:u32=256",
            "width:256",
            "height:256",
        ] {
            check!(
                !compact_contains(&code, forbidden),
                "C: dead/inert vendor feature must not be implemented: {forbidden}"
            );
        }
        check!(
            !compact_contains(&code, "fnsound_texture"),
            "C: SoundTexture must remain the reusable helper, not an inline scene function"
        );
        if let Some(body) = fn_body(&code, "rebuild") {
            check!(
                compact_contains(body, "TextureView(sound.view())")
                    && compact_contains(body, "address_mode_u:wgpu::AddressMode::Repeat")
                    && compact_contains(body, "address_mode_v:wgpu::AddressMode::Repeat")
                    && compact_contains(body, "mag_filter:wgpu::FilterMode::Linear")
                    && compact_contains(body, "min_filter:wgpu::FilterMode::Linear")
                    && compact_contains(body, "mipmap_filter:wgpu::MipmapFilterMode::Nearest"),
                "C: SoundTexture1 needs the shared Repeat/Linear sampler and sound.view() binding"
            );
        } else {
            check!(
                false,
                "C: missing rebuild implementation for sampler binding checks"
            );
        }

        // C/Bloom1 — XML 238-259 and P9Scene.js:963-968. Only Radius, Intensity,
        // Threshold, and ThresholdSmoothness are read. Intensity is absent from
        // the XML, so node.num('Intensity', 1) supplies 1.0. BlurIterations,
        // WeightScale, Weight1..7, and Tint1..7 are vestigial and stay inert.
        for needle in [
            "BLOOM_VENDOR_RADIUS:f32=5.0;",
            "BLOOM_THRESHOLD:f32=0.0;",
            "BLOOM_THRESHOLD_SMOOTHNESS:f32=0.5;",
            "BLOOM_STRENGTH:f32=1.0;",
            "Plane9BloomParams::new(",
            "enable_plane9_bloom(",
            "BLOOM_VENDOR_RADIUS",
            "BLOOM_THRESHOLD_SMOOTHNESS",
            "BLOOM_STRENGTH",
            "BlurIterations",
            "WeightScale",
            "Weight1",
            "Weight7",
            "Tint1",
            "Tint7",
            "vestigial",
        ] {
            check!(
                compact_contains(source, needle),
                "C/Bloom1: lost bloom term {needle}"
            );
        }
        check!(
            compact_match_count(source, "BLOOM_STRENGTH") >= 2
                && compact_match_count(source, "BLOOM_THRESHOLD") >= 2,
            "C/Bloom1: default intensity 1.0 and threshold 0 must be consumed, not merely recorded"
        );
        check!(
            compact_contains(source, "Intensity")
                && (compact_contains(source, "default 1.0")
                    || compact_contains(source, "defaults to 1.0")
                    || compact_contains(source, "node.num('Intensity', 1)")),
            "C/Bloom1: the absent Intensity port must be documented as vendor default 1.0"
        );
        for family in ["Weight", "Tint"] {
            for index in 1..=7 {
                let port = format!("{family}{index}");
                check!(
                    compact_contains(source, &port),
                    "C/Bloom1: vestigial port {port} must be recorded"
                );
            }
        }
        let uses_shared_plane9_bloom = compact_contains(&code, "Plane9BloomParams::new(")
            && compact_contains(&code, "enable_plane9_bloom(")
            && compact_match_count(source, "BLOOM_VENDOR_RADIUS") >= 2
            && compact_match_count(source, "BLOOM_THRESHOLD_SMOOTHNESS") >= 2;
        let uses_shared_unreal_bloom = compact_contains(&code, "UnrealBloomPass::new(")
            && compact_contains(source, "BLOOM_RADIUS:f32=0.85;")
            && compact_contains(&code, "set_radius(BLOOM_RADIUS)")
            && compact_contains(&code, "set_threshold(BLOOM_THRESHOLD)")
            && compact_contains(&code, "set_strength(BLOOM_STRENGTH)")
            && compact_match_count(source, "BLOOM_THRESHOLD_SMOOTHNESS") >= 2;
        check!(
            uses_shared_plane9_bloom || uses_shared_unreal_bloom,
            "C/Bloom1: consume radius 5, threshold 0, smoothness 0.5, and default intensity 1.0 through a shared bloom pass"
        );

        // D — Shader1 (XML 129-237): the 50,000-point wave field. Every term is
        // pinned, including the screen-space point size and additive One/One state.
        for needle in [
            "COLOR1:[f32;4]=[0.314,0.863,0.129,1.0];",
            "SHADER1_DEPTH_TEST:bool=false;",
            "SHADER1_DEPTH_WRITE:bool=false;",
            "SHADER1_SRC_BLEND:u32=1;",
            "SHADER1_DST_BLEND:u32=1;",
            "SHADER1_CULL:u32=0;",
            "SHADER2_DEPTH_TEST:bool=false;",
            "SHADER2_DEPTH_WRITE:bool=false;",
            "SHADER2_SRC_BLEND:u32=1;",
            "SHADER2_DST_BLEND:u32=0;",
            "SHADER2_CULL:u32=0;",
        ] {
            check!(
                compact_contains(source, needle),
                "D: lost colour/render-state constant {needle}"
            );
        }
        check!(
            compact_match_count(&code, "depth_stencil:None") >= 2
                && compact_match_count(&code, "cull_mode:None") >= 2,
            "D: both point and composite passes must be depthless and unculled"
        );
        check!(
            compact_contains(&code, "wgpu::BlendState::ADDITIVE")
                || compact_contains(
                    &code,
                    "color:wgpu::BlendComponent{src_factor:wgpu::BlendFactor::One,dst_factor:wgpu::BlendFactor::One,operation:wgpu::BlendOperation::Add}"
                ),
            "D: Shader1 must use One/One additive colour blending"
        );
        check!(
            !compact_contains(&code, "wgpu::BlendState::ALPHA_BLENDING"),
            "D: Shader1 must not use alpha blending"
        );
        check!(
            compact_contains(&code, "UnitQuad::VERTEX_COUNT")
                && compact_contains(&code, "0..TOTAL_POINTS"),
            "D: 50,000 point sprites must be drawn as instanced screen-space quads"
        );

        let wave = raw_const(source, "WAVES_OF_SIN_WGSL");
        check!(wave.is_some(), "D: missing WAVES_OF_SIN_WGSL literal");
        if let Some(wave) = wave.as_deref() {
            for needle in [
                "@builtin(instance_index) instance: u32",
                "let instance_float = f32(instance);",
                "let points_per_wave = 1000.0;",
                "let wave_delta = wave_nr / 50.0;",
                "let s2 = textureSampleLevel(g_texture1, g_sampler, vec2<f32>(0.5, wave_delta), 0.0).rgb;",
                "let strength = (s2.r * 5.0) / (s2.r * 5.0 + 1.0);",
                "wp.x = ((point_delta * 2.0 - 1.0) * (1.0 + wave_delta * 15.0)) * 10.0;",
                "wp.y = p9_noise(vec3<f32>(point_delta * 3.0 + u.in2.y * (1.0 + p9_rand(wave_delta + 0.1) * 2.0), wave_delta * 10.0, u.in2.y)) * (strength * 5.0) - 2.0;",
                "wp.z = -wave_nr * 2.0 - 5.0;",
                "let bright = p9_smoothstep(0.7, 1.0, p9_noise(vec3<f32>(point_delta * 100.0 + u.time * 0.0, wave_nr * 100.0, u.in2.y * (0.1 + 1.0)))) * 5.0 * u.in2.x;",
                "var diffuse = mix(u.color1, u.color1 * 15.0, bright);",
                "let viewdir = u.camera_position.xyz - worldpos;",
                "let worldpos = (u.model * vec4<f32>(wp, 1.0)).xyz;",
                "let viewpos = (u.view * vec4<f32>(wp, 1.0)).xyz;",
                "let depth = (-viewpos.z - u.z_near) / (u.z_far - u.z_near);",
                "let focus_plane = u.in3.x + u.z_near;",
                "let camera_coc_scale = u.in3.y * (u.resolution.y / 1080.0);",
                "let coc_bias = camera_coc_scale * (1.0 - focus_plane / u.z_near);",
                "let coc_scale = camera_coc_scale * focus_plane * (u.z_far - u.z_near) / (u.z_far * u.z_near);",
                "let coc = min(10.0, abs(depth * coc_scale + coc_bias));",
                "let dist_to_cam = length(viewdir);",
                "let size_px = 2.0 + coc;",
                "let brightness = 1.0 / (size_px * size_px);",
                "diffuse *= 50.0;",
                "diffuse *= 1.0 + 10.0 * p9_saturate(sin(u.time * 10.0 - wave_nr)) * u.in2.x;",
                "diffuse *= brightness;",
                "diffuse *= p9_bump(point_delta, 0.1, 0.9, 0.1);",
                "diffuse.rgb = fog(diffuse.rgb, dist_to_cam, 0.15);",
                "diffuse.a = brightness;",
                "let clip = u.view_proj * vec4<f32>(wp, 1.0);",
                "let offset = corner * size_px * 2.0 / u.resolution;",
                "clip.xy + offset * clip.w",
                "let p = in.tex * 2.0 - 1.0;",
                "return vec4<f32>(in.diffuse.rgb * p9_smoothstep(1.0, 0.8, length(p)), in.diffuse.a);",
            ] {
                check!(
                    compact_contains(wave, needle),
                    "D: wave shader lost {needle}"
                );
            }
            for dead in ["total_points", "wnormal", "i_normal"] {
                check!(
                    !compact_contains(wave, dead),
                    "D: the vendor's unread {dead} local/varying must stay unimplemented"
                );
            }
            check!(
                compact_contains(wave, "floor(instance_float / points_per_wave)")
                    && (compact_contains(wave, "fract(instance_float / points_per_wave)")
                        || compact_contains(wave, "instance_float / points_per_wave - wave_nr")),
                "D: modf(instanceFloat/pointsPerWave, waveNr) must map to floor + fractional part"
            );
            check!(
                compact_contains(wave, "@location(2) corner: vec2<f32>")
                    && compact_contains(wave, "@location(3) uv: vec2<f32>")
                    && compact_contains(wave, "out.tex = uv;"),
                "D: gl_PointCoord must map to the instanced quad UV"
            );
            check!(
                !compact_contains(wave, "fov_scale")
                    && !compact_contains(wave, "size_px * (u.resolution.y")
                    && !compact_contains(wave, "size_px*u.resolution.y"),
                "D: gl_PointSize is screen-space pixels, not a world-space attenuated billboard"
            );
            check!(
                compact_match_count(wave, "textureSampleLevel(") == 1
                    && compact_match_count(wave, "textureSample(") == 0
                    && compact_match_count(wave, "texture(") == 0,
                "D: the SoundTexture sample must be the sole explicit-LOD-0 vertex texture read"
            );
            let without_p9_smoothstep = compact_contains(wave, "p9_smoothstep(")
                && !compact_contains(&wave.replace("p9_smoothstep(", ""), "smoothstep(");
            check!(
                without_p9_smoothstep,
                "D: the reversed-edge fragment smoothstep must use p9_smoothstep, never WGSL's builtin"
            );
            check!(
                !compact_contains(wave, "lens"),
                "A/D: Lens may magnify the camera projection only, never point size or CoC"
            );
            match shader_fn_scope(wave, "fog") {
                Some(fog) => {
                    for needle in [
                        "fn fog(",
                        "1.0 - exp(-dist_to_cam * density)",
                        "mix(org_col, vec3<f32>(0.0), fog_amount)",
                    ] {
                        check!(compact_contains(fog, needle), "D/fog: lost {needle}");
                    }
                }
                None => check!(false, "D/fog: missing shader-local function scope"),
            }
            check!(
                validates_with_plane9_preludes(wave),
                "D: WAVES_OF_SIN_WGSL does not parse/validate with the Plane9 preludes"
            );
        }

        // E — Shader2 (XML 32-79): quarter-gain bloom + raw RTT, lift/gamma/gain,
        // ACES, then screen blend over PreviousLayer. The final alpha is 0.0.
        let composite = raw_const(source, "WAVES_OF_SIN_COMPOSITE_WGSL");
        check!(
            composite.is_some(),
            "E: missing WAVES_OF_SIN_COMPOSITE_WGSL literal"
        );
        if let Some(composite) = composite.as_deref() {
            for needle in [
                "let c1 = textureSampleLevel(g_texture1, g_sampler, tex, 0.0) * 0.25;",
                "let c2 = textureSampleLevel(g_texture2, g_sampler, tex, 0.0);",
                "let bg = textureSampleLevel(g_texture3, g_sampler, tex, 0.0);",
                "let c = c1 + c2;",
                "c.rgb = p9_saturate3(p9_lift_gamma_gain(c.rgb, vec3<f32>(0.0), vec3<f32>(1.5, 1.5, 1.0), vec3<f32>(1.2, 1.2, 1.5)));",
                "c = vec4<f32>(p9_tonemap_aces(c.rgb), 1.0);",
                "return vec4<f32>(p9_blend_screen(bg.rgb, c.rgb), 0.0);",
            ] {
                check!(
                    compact_contains(composite, needle),
                    "E: composite shader lost {needle}"
                );
            }
            for forbidden in [
                "* 2.0",
                "p9_brightness_saturation_contrast(",
                "max(c1.a, c2.a)",
                "return vec4<f32>(p9_blend_screen(bg.rgb, c.rgb), 1.0);",
            ] {
                check!(
                    !compact_contains(composite, forbidden),
                    "E: composite retained Music Waves/non-vendor term {forbidden}"
                );
            }
            check!(
                compact_match_count(composite, "textureSampleLevel(") == 3
                    && compact_match_count(composite, "textureSample(") == 0
                    && compact_match_count(composite, "texture(") == 0,
                "E: all three composite texture reads must use explicit LOD 0"
            );
            check!(
                validates_with_plane9_preludes(composite),
                "E: WAVES_OF_SIN_COMPOSITE_WGSL does not parse/validate with the Plane9 preludes"
            );
        }
        for needle in [
            "g_texture1",
            "g_texture2",
            "g_texture3",
            "Bloom1",
            "RenderToTexture1",
        ] {
            check!(
                compact_contains(source, needle),
                "E: composite binding lost {needle}"
            );
        }
        check!(
            compact_contains(&code, "blend:None")
                || compact_contains(
                    &code,
                    "color:wgpu::BlendComponent{src_factor:wgpu::BlendFactor::One,dst_factor:wgpu::BlendFactor::Zero,operation:wgpu::BlendOperation::Add}"
                ),
            "E: Shader2 SrcBlend 1 / DstBlend 0 must replace the target"
        );

        // E/shared P9Glsl — port _bump (P9Glsl.js:412-415) and _liftGammaGain
        // (:486-489) once in the common prelude. Scope each formula to its own
        // function so a sibling helper cannot satisfy the assertion.
        match shader_fn_scope(P9_COMMON_WGSL, "p9_bump") {
            Some(bump) => {
                for needle in [
                    "fn p9_bump(",
                    "p9_smoothstep(start - width, start, value) - p9_smoothstep(end, end + width, value)",
                ] {
                    check!(compact_contains(bump, needle), "E/p9_bump: lost {needle}");
                }
                check!(
                    !compact_contains(&bump.replace("p9_smoothstep(", ""), "smoothstep("),
                    "E/p9_bump: must preserve GLSL smoothstep semantics through p9_smoothstep"
                );
            }
            None => check!(false, "E/p9_bump: missing shared function scope"),
        }
        match shader_fn_scope(P9_COMMON_WGSL, "p9_lift_gamma_gain") {
            Some(lift) => {
                check!(
                    compact_contains(lift, "fn p9_lift_gamma_gain(")
                        && compact_contains(
                            lift,
                            "pow(gain * (col + lift * (vec3<f32>(1.0) - col)), gamma)"
                        ),
                    "E/p9_lift_gamma_gain: lost the vendor pow(gain*(col+lift*(1-col)), gamma) formula"
                );
            }
            None => check!(false, "E/p9_lift_gamma_gain: missing shared function scope"),
        }
        let helper_probe = format!(
            "{P9_COMMON_WGSL}\n{}\n",
            r#"
@fragment
fn fs_main() -> @location(0) vec4<f32> {
  let bumped = p9_bump(0.5, 0.1, 0.9, 0.1);
  let lifted = p9_lift_gamma_gain(
    vec3<f32>(0.2, 0.4, 0.6),
    vec3<f32>(0.0),
    vec3<f32>(1.5, 1.5, 1.0),
    vec3<f32>(1.2, 1.2, 1.5),
  );
  return vec4<f32>(lifted, bumped);
}
"#
        );
        check!(
            naga::front::wgsl::parse_str(&helper_probe)
                .ok()
                .and_then(|module| {
                    naga::valid::Validator::new(
                        naga::valid::ValidationFlags::all(),
                        naga::valid::Capabilities::all(),
                    )
                    .validate(&module)
                    .ok()
                })
                .is_some(),
            "E: p9_bump/p9_lift_gamma_gain do not parse/validate in the shared prelude"
        );
        check!(
            compact_contains(wave.as_deref().unwrap_or_default(), "p9_bump(")
                && compact_contains(
                    composite.as_deref().unwrap_or_default(),
                    "p9_lift_gamma_gain("
                ),
            "E: the scene must consume both newly ported shared helpers"
        );

        // F — Shader1.Expression (XML 231-235): bass advances perm, permrand*10
        // offsets t, damping-1 mid drives the flash gain, and t drives noise time.
        // in1 is unused. Both band reads and the deterministic recurrence are pinned.
        for needle in [
            "PERMRAND:f32=1.0;",
            "SHADER1_IN3:[f32;4]=[0.0122,67.638,0.0,0.0];",
            "perm = perm + ((min(1, band(-1, 0, 0, 0.1)*5+0.01))*deltatime);",
            "t = perm + permrand*10;",
            "in2.x = band(-1, 1, 1, 0)*10.0;",
            "in2.y = t;",
            "waves_of_sin_drive",
            "unused",
        ] {
            check!(
                compact_contains(source, needle),
                "F: expression/provenance lost {needle}"
            );
        }
        if let Some(body) = fn_body(&code, "waves_of_sin_drive") {
            for needle in [
                "(phase_band*5.0+0.01).min(1.0)*dt",
                "permrand*10.0",
                "flash_band*10.0",
            ] {
                check!(
                    compact_contains(body, needle),
                    "F: waves_of_sin_drive lost {needle}"
                );
            }
        } else {
            check!(false, "F: missing waves_of_sin_drive implementation");
        }
        if let Some(body) = fn_body(&code, "update") {
            for needle in [
                "self.audio.band(-1,0,0,0.1)",
                "self.audio.band(-1,1,1,0.0)",
                "self.uniforms.in2=[drive.in2[0],drive.in2[1],0.0,0.0]",
                "self.uniforms.in3=SHADER1_IN3",
            ] {
                check!(compact_contains(body, needle), "F: update lost {needle}");
            }
            check!(
                !compact_contains(body, "rand("),
                "F: the expression must be deterministic; p9_rand belongs only to the shader's pure hash"
            );
        } else {
            check!(false, "F: missing update implementation");
        }
        for shader in [wave.as_deref(), composite.as_deref()]
            .into_iter()
            .flatten()
        {
            check!(
                !compact_contains(shader, "u.in1") && !compact_contains(shader, "gIn1"),
                "F: in1 is unused and must stay unread by both shaders"
            );
        }
    }

    // G — 320x180 fallback-adapter smoke. 50,000 additive quads are expensive,
    // so use three frames and report init/render/readback timings. The composite
    // writes alpha 0.0: lit means nonzero RGB, never any_lit's alpha gate.
    if let Some(index) = waves_of_sin_index {
        check!(
            source.is_some(),
            "G: registered Waves of Sin has no scene source {}",
            scene_path.display()
        );
        if let Some((device, queue)) = gpu() {
            const WIDTH: u32 = 320;
            const HEIGHT: u32 = 180;
            let started = Instant::now();
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Waves of Sin registry lookup");
            let mut ctx = SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            ctx.resize(WIDTH, HEIGHT);
            scene.init(&mut ctx);
            scene.resize(WIDTH, HEIGHT);
            let mut repeat_ctx =
                SceneContext::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            repeat_ctx.resize(WIDTH, HEIGHT);
            repeat.init(&mut repeat_ctx);
            repeat.resize(WIDTH, HEIGHT);
            eprintln!(
                "Plane9 Waves of Sin GPU init (320x180, two instances): {:?}",
                started.elapsed()
            );

            let mut payload = BandPayload::silent(44_100.0);
            payload.lanes[0] = 0.2;
            payload.lanes[4] = 0.8;
            payload.byte_frequency_data = vec![180; 1024];
            payload.freq_mag.fill(0.5);
            let mut bridge = AudioBridge::new();
            let mut repeat_bridge = AudioBridge::new();
            let target = TestTarget::new(&device, WIDTH, HEIGHT);
            let repeat_target = TestTarget::new(&device, WIDTH, HEIGHT);
            let render = |scene: &mut Box<dyn Point10Scene>,
                          bridge: &mut AudioBridge,
                          target: &TestTarget,
                          dt: f32,
                          label: &str| {
                let frame_started = Instant::now();
                bridge.update(dt, Some(&payload), &Default::default());
                scene.update(dt, bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-waves-of-sin-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                eprintln!(
                    "Plane9 Waves of Sin GPU {label} (320x180, 50000 quads): {:?}",
                    frame_started.elapsed()
                );
                pixels
            };

            let first = render(&mut scene, &mut bridge, &target, 0.0, "frame 1 (dt=0)");
            let repeated = render(
                &mut repeat,
                &mut repeat_bridge,
                &repeat_target,
                0.0,
                "fresh frame 1 (dt=0)",
            );
            let second = render(
                &mut scene,
                &mut bridge,
                &target,
                1.0 / 60.0,
                "frame 2 (dt=1/60)",
            );
            let first_lit = rgb_lit_pixels(&first);
            let repeated_lit = rgb_lit_pixels(&repeated);
            let second_lit = rgb_lit_pixels(&second);
            eprintln!(
                "Plane9 Waves of Sin RGB-lit pixels: first={first_lit} repeat={repeated_lit} second={second_lit}; alpha_zero={}/{}/{}",
                all_alpha_zero(&first),
                all_alpha_zero(&repeated),
                all_alpha_zero(&second)
            );
            check!(first_lit > 0, "G: Waves of Sin frame 1 has no nonzero RGB");
            check!(second_lit > 0, "G: Waves of Sin frame 2 has no nonzero RGB");
            check!(
                first == repeated,
                "G: Waves of Sin frame 1 is not byte-for-byte deterministic across fresh instances"
            );
            check!(
                first != second,
                "G: Waves of Sin later frame did not evolve"
            );
            check!(
                all_alpha_zero(&first) && all_alpha_zero(&repeated) && all_alpha_zero(&second),
                "G: Shader2's final alpha-0.0 composite must keep every presented pixel alpha zero"
            );
        }
    } else {
        failures.push(format!(
            "G: cannot run the 320x180 GPU smoke because {SCENE_NAME} is not registered"
        ));
    }

    assert!(
        failures.is_empty(),
        "Plane9 Waves of Sin fidelity contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
