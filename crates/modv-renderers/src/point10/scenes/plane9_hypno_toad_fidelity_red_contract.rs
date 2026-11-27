//! RED contract for `Psychedelic/HypnoToad.xml`. The scene is resolved through
//! the registry and its future source/helper are read at runtime, so this
//! compiles before `Plane9 Hypno Toad`, `HypnoToad`, or `p9_tolinear` exists.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::point10::audio_bridge::{AudioBridge, BandPayload, P10Audio, RESPONSIVENESS_PROPS};
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_DUAL_TEXTURE_WGSL, PLANE9_QUAD_UNIFORMS_WGSL, PLANE9_TEXTURE_WGSL,
    PLANE9_TONEMAP_WGSL, PLANE9_UNIFORMS_WGSL,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::test_support::{
    TestTarget, any_lit, compact_contains, compact_match_count, gpu,
};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Hypno Toad";
const LEGACY_NAME: &str = "Point10 Hypno Toad";
const HYPNO_TOAD_LICENSE: &str = concat!("CC BY-", "NC-", "SA 3.0");
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
/// comments are allowed to explain which vendor nodes are deliberately absent.
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

fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let declaration = source.find(&format!("fn {name}"))?;
    let open = declaration + source[declaration..].find('{')?;
    let mut depth = 0usize;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[open + 1..open + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Validate a scene-owned WGSL literal with the shared Plane9 preludes. The
/// quad prelude is accepted for a fragment-only scene literal, while the empty
/// and uniform preludes allow a scene-owned vertex/uniform declaration.
fn validates_with_plane9_preludes(shader: &str) -> bool {
    let common = format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_TONEMAP_WGSL}\n");
    let preludes = [
        String::new(),
        PLANE9_UNIFORMS_WGSL.to_string(),
        PLANE9_QUAD_UNIFORMS_WGSL.to_string(),
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

/// CPU mirror of the vendor polynomial, used for the endpoint value contract.
/// The production WGSL body itself is pinned independently below.
fn p9_tolinear(srgb: f32) -> f32 {
    srgb * (srgb * (srgb * 0.305_306_02 + 0.682_171_1) + 0.012_522_878)
}

/// The clone's deliberately unclamped red expression.
fn clone_red(pos_x: f32, pos_y: f32) -> f32 {
    1.0 - pos_x.hypot(pos_y) / 2.0
}

fn abs_diff_below(value: f32, expected: f32, epsilon: f32) -> bool {
    (value - expected).abs() < epsilon
}

#[test]
fn plane9_hypno_toad_fidelity_red_contract() {
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

    // A — XML 2-6 and the registry contract: the seventh XML batch appends one
    // canonical Plane9/Dahl scene after Music Waves.
    check!(
        scenes.len() == 51,
        "A: expected 51 Point10 scenes, got {}",
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
        names.last() == Some(&SCENE_NAME),
        "A: Hypno Toad must be the deterministic roster tail, got {:?}",
        names.last()
    );
    check!(
        names.get(names.len().saturating_sub(2)) == Some(&"Plane9 Music Waves"),
        "A: Hypno Toad must follow Plane9 Music Waves without reordering the roster"
    );
    check!(
        PLANE9_SCENE_NAMES.len() == 33,
        "A: expected 33 Plane9 scene names, got {}",
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
        PLANE9_SCENE_NAMES.last() == Some(&SCENE_NAME),
        "A: PLANE9_SCENE_NAMES must end with {SCENE_NAME}"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(2))
            == Some(&"Plane9 Music Waves"),
        "A: PLANE9_SCENE_NAMES must append Hypno Toad after Music Waves"
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

    // A — the parity fixture and generated catalogue move from 50/501 to
    // 51/502. XML line 6 is the 3.0 licence arm, not the 4.0 arm.
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("tests/fixtures/module-parity.json"))
            .expect("read module-parity.json"),
    )
    .expect("parse module-parity.json");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(51),
        "A: manifest point10 count is {:?}, expected 51",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(502),
        "A: manifest total is {:?}, expected 502",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 51,
        "A: manifest point10 array has {} names, expected 51",
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
        manifest_names.last() == Some(&SCENE_NAME)
            && manifest_names.get(manifest_names.len().saturating_sub(2))
                == Some(&"Plane9 Music Waves"),
        "A: manifest must append {SCENE_NAME} after Plane9 Music Waves"
    );

    let catalogue: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
            .expect("read docs/modules/modules.json"),
    )
    .expect("parse docs/modules/modules.json");
    check!(
        catalogue["counts"]["point10"].as_u64() == Some(51),
        "A: catalogue point10 count is {:?}, expected 51",
        catalogue["counts"]["point10"]
    );
    check!(
        catalogue["counts"]["total"].as_u64() == Some(502),
        "A: catalogue total is {:?}, expected 502",
        catalogue["counts"]["total"]
    );
    check!(
        catalogue["modules"].as_array().map(Vec::len) == Some(502),
        "A: catalogue modules array must contain 502 entries"
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
        let prop_names: Vec<&str> = module["props"]
            .as_array()
            .expect("catalogue Hypno Toad props")
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
        .filter(|line| line.starts_with("| Hypno Toad |"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if row.contains("hypno_toad.rs")
                && row.contains(HYPNO_TOAD_LICENSE)
                && row.contains("Joakim Dahl") => {}
        [row] => check!(false, "A: wrong Hypno Toad notices row {row:?}"),
        [] => check!(false, "A: missing Hypno Toad notices row"),
        _ => check!(
            false,
            "A: duplicate Hypno Toad notices rows {notice_rows:?}"
        ),
    }
    let ui_model = fs::read_to_string(repo_root.join("crates/modv-ui/src/model.rs"))
        .expect("read modv-ui model.rs");
    let v3_arm = ui_model
        .split("// Plane9 ports whose own headers state 3.0")
        .nth(1)
        .and_then(|arm| arm.split("// Plane9 shader effects").next())
        .unwrap_or_default();
    let v4_arm = ui_model
        .split("// Plane9 ports whose own headers state 4.0.")
        .nth(1)
        .and_then(|arm| arm.split("// Point10 variants").next())
        .unwrap_or_default();
    check!(
        v3_arm.contains("\"Hypno Toad\""),
        "A: module_license exact-3.0 arm is missing Hypno Toad"
    );
    check!(
        !v4_arm.contains("\"Hypno Toad\""),
        "A: Hypno Toad must not be classified in the 4.0 licence arm"
    );
    check!(
        compact_contains(&ui_model, "checked.len(),51,"),
        "A: UI licence provenance count must move from 50 to 51"
    );

    // A — the app wrapper must construct the canonical name and delegate every
    // Point10Scene method plus apply_props, without this RED test naming the
    // not-yet-existing Rust type.
    let app_registry = production_source(&repo_root.join("crates/modv-app/src/module_registry.rs"))
        .expect("read app module registry");
    check!(
        compact_contains(
            &app_registry,
            "usemodv_renderers::point10::scenes::hypno_toad::HypnoToad;"
        ),
        "A: app registry is missing the HypnoToad import"
    );
    check!(
        compact_contains(&app_registry, "HypnoToad(HypnoToad)"),
        "A: Point10SceneKind is missing HypnoToad(HypnoToad)"
    );
    check!(
        app_registry.contains("\"Plane9 Hypno Toad\" => Self::HypnoToad(HypnoToad::new())"),
        "A: Point10SceneKind::create is missing Plane9 Hypno Toad"
    );
    check!(
        compact_match_count(&app_registry, "Self::HypnoToad(scene)") == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_match_count(&app_registry, "Self::HypnoToad(scene)")
    );
    let app_registry_with_tests =
        fs::read_to_string(repo_root.join("crates/modv-app/src/module_registry.rs"))
            .expect("read app module registry with tests");
    let wrapper_order = fn_body(
        &app_registry_with_tests,
        "point10_definitions_match_the_wrapper_modules",
    )
    .unwrap_or_default();
    let music_position = wrapper_order.find("\"Plane9 Music Waves\"");
    let hypno_position = wrapper_order.find("\"Plane9 Hypno Toad\"");
    check!(
        music_position.is_some_and(|music| { hypno_position.is_some_and(|hypno| music < hypno) }),
        "A: wrapper-order list must append Plane9 Hypno Toad after Plane9 Music Waves"
    );
    if let Some(hypno) = hypno_position {
        let tail = &wrapper_order[hypno + "\"Plane9 Hypno Toad\"".len()..];
        let tail = tail.trim_start();
        let tail = tail.strip_prefix(',').unwrap_or(tail).trim_start();
        check!(
            tail.starts_with(']'),
            "A: Plane9 Hypno Toad must be the wrapper-order list tail"
        );
    }

    // A/props — the vendor tree has no sceneProps.js and no per-scene override
    // for Hypno Toad, so the schema is exactly responsiveness + Lens.
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
    let projection = perspective(45.0_f32.to_radians(), 1280.0 / 720.0, 0.1, 1000.0);
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
    let hypno_toad_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = hypno_toad_index {
        check!(
            scenes[index].props() == expected_props,
            "A: Hypno Toad props must be responsiveness + Lens only; got {:?}",
            scenes[index].props()
        );
    }

    // B-G — read the not-yet-existing module at runtime. All constants and
    // shader contracts below cite the XML/JS lines they transcribe.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/hypno_toad.rs");
    let source = production_source(&scene_path);
    if source.is_none() {
        for group in ["B", "C", "D", "E", "F"] {
            failures.push(format!(
                "{group}: missing scene source {}",
                scene_path.display()
            ));
        }
    }
    if let Some(source) = source {
        let code = rust_code(&source);

        // A — XML 2-6: provenance stays visible in the port's source.
        for needle in [
            "HypnoToad.xml",
            "Joakim Dahl",
            "Dark yet, colorfull and hypnotic",
            "psychedelic",
        ] {
            check!(source.contains(needle), "A: scene source lost {needle:?}");
        }
        check!(
            source.contains("SceneType=\"256\"")
                && source.to_ascii_lowercase().contains("standalone"),
            "A: SceneType 256 must be recorded as the vendor standalone pool bit"
        );
        check!(
            compact_contains(
                &source,
                "constHYPNO_TOAD_LICENSE:&str=concat!(\"CCBY-\",\"NC-\",\"SA3.0\");"
            ),
            "A: licence marker must use the split concat! 3.0 form"
        );
        // Only the machine-checked marker must be split; provenance docs may
        // legitimately spell out the full licence for readers.
        let licence_declaration = source
            .lines()
            .find(|line| line.contains("const HYPNO_TOAD_LICENSE"))
            .unwrap_or_default();
        check!(
            licence_declaration.contains("concat!("),
            "A: the licence declaration must stay in the split concat! form"
        );
        check!(
            !compact_contains(licence_declaration, NC_SA_MARKER),
            "A: the licence declaration must not contain contiguous scanner text"
        );
        check!(
            !compact_contains(licence_declaration, "SA 4.0"),
            "A: the licence declaration must not claim 4.0"
        );

        // B — Screen1 (XML 10-17). P9Scene.js:560-580 applies CamRot after
        // lookAt as local-space rotateX/rotateY/rotateZ, in that order.
        for needle in [
            "CAMERA_POSITION:[f32;3]=[0.0,0.0,5.29];",
            "CAMERA_ROTATION:[f32;3]=[0.0,0.0,0.0];",
            "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,-1.0];",
            "CAMERA_LOOK_AT_IN_WORLD_SPACE:bool=false;",
            "CAMERA_FOV:f32=45.0;",
            "CAMERA_NEAR:f32=0.1;",
            "CAMERA_FAR:f32=1000.0;",
            "SCALE_BY_ASPECT:bool=true;",
        ] {
            check!(
                compact_contains(&source, needle),
                "B: lost camera constant {needle}"
            );
        }
        if let Some(body) = fn_body(&source, "view_proj") {
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
                "ifSCALE_BY_ASPECT",
            ] {
                check!(compact_contains(body, needle), "B: view_proj lost {needle}");
            }
        } else {
            check!(false, "B: missing view_proj implementation");
        }

        // C — RenderObject1 (XML 19-24), CloneExpression1 (28-49), and the
        // graph edges at 111-116. The XML connects RenderObject1.Object, but a
        // RenderObject has no geometry/getObject override (P9Nodes.js:140,
        // 773-776), so resolveObject returns null and CloneExpression falls back
        // to new THREE.PlaneGeometry(1, 1) (P9Nodes.js:1551).
        for needle in [
            "RenderObject1.Object",
            "1x1 plane",
            "CLONE_OBJECTS_X:usize=1500;",
            "CLONE_OBJECTS_Y:usize=1;",
            "CLONE_OBJECTS_Z:usize=1;",
            "ADJUST_FOR_ASPECT:bool=false;",
            "CLONE_CENTER:bool=false;",
            "OBJECT_POSITION:[f32;3]=[0.0,0.0,0.0];",
            "OBJECT_ROTATION:[f32;3]=[0.0,0.0,0.0];",
            "OBJECT_SCALE:[f32;3]=[1.0,1.0,1.0];",
            "OBJECT_COLOR:[f32;4]=[1.0,1.0,1.0,1.0];",
            "CLONE_SCALE:[f32;3]=[0.2,0.2,0.2];",
        ] {
            check!(
                compact_contains(&source, needle),
                "C: lost geometry/clone constant {needle}"
            );
        }
        check!(
            compact_contains(&source, "PERMRAND:f32=1.0;")
                || compact_contains(&source, "PERM_RAND:f32=1.0;"),
            "C: permrand must use the repository's 1.0 convention"
        );
        check!(
            !compact_contains(&code, "build_box(")
                && !compact_contains(&code, "build_ring(")
                && !compact_contains(&code, "MeshObject"),
            "C: Hypno Toad must not acquire a mesh node or mesh builder"
        );
        check!(
            compact_contains(&code, "Plane9QuadPass")
                || (compact_contains(&code, "UnitQuad")
                    && compact_contains(&code, "VertexStepMode::Instance")),
            "C: the fallback geometry must be an instanced unit quad"
        );
        for expression_line in [
            "pos.x = cos(x*(in1+in2*100)*0.005+x)-cos(x);",
            "pos.y = sin(x*(in1+in2*100)*0.005+x)+sin(x);",
            "col.r = 1.0-length(pos.x, pos.y)/2;",
            "col.a = x/numobjectsx;",
            "scale.x = scale.y = scale.z = 0.2;",
        ] {
            check!(
                source.contains(expression_line),
                "C: clone expression lost exact line {expression_line:?}"
            );
        }
        if let Some(body) = fn_body(&source, "hypno_toad_instances") {
            for needle in [
                "Vec::with_capacity(CLONE_OBJECTS_X)",
                "0..CLONE_OBJECTS_X",
                "letx=indexasf32+1.0;",
                "letdrive=in1+in2*100.0;",
                "(x*drive*0.005+x).cos()-x.cos()",
                "(x*drive*0.005+x).sin()+x.sin()",
                "1.0-pos_x.hypot(pos_y)/2.0",
                "x/CLONE_OBJECTS_Xasf32",
                "[red,0.0,0.0,alpha]",
                "scale:[CLONE_SCALE[0],CLONE_SCALE[1]]",
            ] {
                check!(
                    compact_contains(body, needle),
                    "C: hypno_toad_instances lost {needle}"
                );
            }
            check!(
                !compact_contains(body, "aspect"),
                "C: AdjustForAspect false must keep exactly 1500 instances at every aspect"
            );
            for forbidden in ["red.max(0.0)", "max(0.0,red)", "red.clamp(", "clamp(red,"] {
                check!(
                    !compact_contains(body, forbidden),
                    "C: clone red must remain unclamped; found {forbidden}"
                );
            }
        } else {
            check!(false, "C: missing hypno_toad_instances implementation");
        }
        let first_alpha = 1.0_f32 / 1500.0_f32;
        check!(
            abs_diff_below(first_alpha, 0.000_666_666_7, 1.0e-9),
            "C: first instance alpha must be 1/1500, got {first_alpha}"
        );
        check!(
            abs_diff_below(1500.0_f32 / 1500.0_f32, 1.0, f32::EPSILON),
            "C: last instance alpha must be 1.0"
        );
        // The real spirograph inputs are differences of unit vectors, so their
        // length is <= 2 for finite values. Still pin the vendor's unclamped
        // formula with a synthetic length > 2 rather than allowing a sanitising
        // max(0) to hide the expression's actual behaviour.
        let synthetic_red = clone_red(2.0, 2.0);
        let synthetic_red_is_negative = synthetic_red < 0.0;
        check!(
            synthetic_red_is_negative,
            "C: col.r must be allowed to go negative when length > 2"
        );

        // D — Expression1 (XML 96-107) is live: Out1/Out2 feed
        // CloneExpression1.In1/In2 at XML 114-115.
        for expression in [
            "perm = perm + (beat(0.03)*2+0.001)*deltatime;",
            "out1 = perm;",
            "out2 = permrand;",
        ] {
            check!(
                compact_contains(&source, expression),
                "D: Expression1 lost {expression}"
            );
        }
        check!(
            abs_diff_below(P10Audio::new().beat(0.03), 0.03, 1.0e-6),
            "D: beat(0.03) must use 0.03 when no audio is available"
        );
        check!(
            compact_contains(&source, "HypnoToadDrive"),
            "D: missing HypnoToadDrive result type"
        );
        if let Some(body) = fn_body(&source, "hypno_toad_drive") {
            for needle in [
                "(beat*2.0+0.001)*dt",
                "letnext=perm+",
                "out1:next",
                "out2:permrand",
            ] {
                check!(compact_contains(body, needle), "D: drive lost {needle}");
            }
        } else {
            check!(false, "D: missing hypno_toad_drive implementation");
        }
        if let Some(body) = fn_body(&source, "update") {
            for needle in [
                ".beat(0.03)",
                "hypno_toad_drive(",
                "drive.out1",
                "drive.out2",
                "hypno_toad_instances(drive.out1,drive.out2)",
            ] {
                check!(
                    compact_contains(body, needle),
                    "D: live Expression1 drive did not reach the clone expression: {needle}"
                );
            }
        } else {
            check!(false, "D: missing update implementation");
        }

        // E — Shader1 (XML 63-89).
        let shader = raw_const(&source, "HYPNO_TOAD_WGSL");
        check!(shader.is_some(), "E: missing HYPNO_TOAD_WGSL literal");
        if let Some(shader) = shader {
            for needle in [
                "letp=in.tex*2.0-1.0;",
                "letcol=p9_hsv2rgb(in.diffuse.a,1.0,1.0-in.diffuse.a);",
                "varoColor=vec4<f32>(p9_tolinear(col),p9_saturate(1.0-length(p))*in.diffuse.r);",
                "returnoColor;",
            ] {
                check!(
                    compact_contains(&shader, needle),
                    "E: Hypno Toad fragment lost {needle}"
                );
            }
            check!(
                validates_with_plane9_preludes(&shader),
                "E: Hypno Toad WGSL does not parse/validate with the Plane9 preludes"
            );
            if shader.contains("@vertex") {
                for needle in [
                    "u.view_proj*",
                    "out.tex=uv",
                    "out.diffuse=instance_color*u.color1",
                ] {
                    check!(
                        compact_contains(&shader, needle),
                        "E: scene-owned vertex lost {needle}"
                    );
                }
            }
        }
        for expression in [
            "gl_Position = gMVP * iPosition",
            "so.tex = iTexCoord",
            "so.diffuse = iColor*gColor1*gColor",
        ] {
            check!(
                compact_contains(&source, expression),
                "E: Shader1 vertex provenance lost {expression}"
            );
        }
        if !source.contains("@vertex") {
            for needle in [
                "out.position=u.view_proj*world;",
                "out.tex=uv;",
                "out.diffuse=instance_color*u.color1;",
            ] {
                check!(
                    compact_contains(PLANE9_QUAD_UNIFORMS_WGSL, needle)
                        && compact_contains(&code, "Plane9QuadPass"),
                    "E: shared quad vertex path lost {needle}"
                );
            }
        }
        for needle in [
            "SHADER1_DEPTH_TEST:bool=false;",
            "SHADER1_DEPTH_WRITE:bool=false;",
            "SHADER1_SRC_BLEND:u32=6;",
            "SHADER1_DST_BLEND:u32=8;",
            "SHADER1_CULL:u32=2;",
            "wgpu::BlendFactor::SrcAlpha",
            "wgpu::BlendFactor::OneMinusSrcAlpha",
            "wgpu::Face::Back",
            "CLEAR1:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
        ] {
            check!(
                compact_contains(&code, needle),
                "E: lost render-state constant/decoded factor {needle}"
            );
        }
        check!(
            compact_contains(&code, "depth_stencil:None")
                || compact_contains(&code, "depth_stencil_attachment:None"),
            "E: DepthTest/DepthWrite false must produce no depth attachment"
        );
        check!(
            compact_match_count(&code, "CLEAR1") >= 2,
            "E: Clear1 must be declared and used as the single pass clear colour"
        );

        // F — XML has no RTT, bloom, or PreviousLayer nodes. This is one clear
        // plus one instanced draw directly into the caller's target.
        for forbidden in [
            "RenderToTexture",
            "UnrealBloomPass",
            "BloomPass",
            "PreviousLayer",
            "Composer",
            "create_colour_target",
            "g_texture1",
        ] {
            check!(
                !compact_contains(&code, forbidden),
                "F: cheap single-pass scene must not contain {forbidden}"
            );
        }
        check!(
            source.to_ascii_lowercase().contains("single-pass"),
            "F: source must document the vendor's single-pass graph"
        );
        if let Some(body) = fn_body(&source, "render") {
            let passes = compact_match_count(body, "begin_render_pass(");
            let draws =
                compact_match_count(body, ".draw(") + compact_match_count(body, ".draw_indexed(");
            check!(
                (passes == 1 || (passes == 0 && compact_contains(body, "Plane9QuadPass")))
                    && draws == 1,
                "F: expected one instanced draw directly to target, got {passes} passes/{draws} draws"
            );
            check!(
                compact_contains(body, "target") && !compact_contains(body, ".rtt"),
                "F: the sole draw must target the caller's screen view, not an offscreen target"
            );
        } else {
            check!(false, "F: missing render implementation");
        }
    }

    // E — P9Glsl.js:29-30 belongs in the shared Plane9 prelude even while the
    // scene module itself is absent, so the RED failure names both missing pieces.
    for needle in [
        "fnp9_tolinear(srgb:vec3<f32>)->vec3<f32>",
        "srgb*(srgb*(srgb*vec3<f32>(0.305306011)+vec3<f32>(0.682171111))+vec3<f32>(0.012522878))",
    ] {
        check!(
            compact_contains(P9_COMMON_WGSL, needle),
            "E: shared Plane9 prelude lost {needle}"
        );
    }
    check!(
        validates_with_plane9_preludes(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { let c = p9_tolinear(vec3<f32>(0.5)); return vec4<f32>(c, 1.0); }"
        ),
        "E: p9_tolinear does not parse/validate in the shared Plane9 prelude"
    );
    check!(
        abs_diff_below(p9_tolinear(0.0), 0.0, 1.0e-7),
        "E: p9_tolinear(0) must be 0"
    );
    check!(
        abs_diff_below(p9_tolinear(1.0), 1.0, 1.0e-6),
        "E: p9_tolinear(1) must be 1"
    );

    // G — 640x360 fallback-adapter smoke. Frame 1 and the fresh repeat use
    // dt=0 + silence for byte determinism. Frame 2 advances the live drive with
    // the no-audio beat(0.03) fallback, so the spirograph must visibly evolve.
    if let Some(index) = hypno_toad_index {
        if let Some((device, queue)) = gpu() {
            const WIDTH: u32 = 640;
            const HEIGHT: u32 = 360;
            let started = Instant::now();
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Hypno Toad registry lookup");
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
                "Plane9 Hypno Toad GPU init (640x360, two instances): {:?}",
                started.elapsed()
            );

            let silent = BandPayload::silent(44_100.0);
            let mut bridge = AudioBridge::new();
            let mut repeat_bridge = AudioBridge::new();
            let target = TestTarget::new(&device, WIDTH, HEIGHT);
            let repeat_target = TestTarget::new(&device, WIDTH, HEIGHT);
            let render = |scene: &mut Box<dyn Point10Scene>,
                          bridge: &mut AudioBridge,
                          payload: Option<&BandPayload>,
                          target: &TestTarget,
                          dt: f32,
                          label: &str| {
                let frame_started = Instant::now();
                bridge.update(dt, payload, &Default::default());
                scene.update(dt, bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-hypno-toad-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                eprintln!(
                    "Plane9 Hypno Toad GPU {label} (640x360): {:?}",
                    frame_started.elapsed()
                );
                pixels
            };

            let first = render(
                &mut scene,
                &mut bridge,
                Some(&silent),
                &target,
                0.0,
                "frame 1 (dt=0, silent)",
            );
            let repeated = render(
                &mut repeat,
                &mut repeat_bridge,
                Some(&silent),
                &repeat_target,
                0.0,
                "fresh frame 1 (dt=0, silent)",
            );
            let second = render(
                &mut scene,
                &mut bridge,
                None,
                &target,
                0.1,
                "frame 2 (dt=0.1, beat fallback)",
            );
            check!(any_lit(&first), "G: Hypno Toad frame 1 is empty at 640x360");
            check!(
                any_lit(&second),
                "G: Hypno Toad frame 2 is empty at 640x360"
            );
            check!(
                first == repeated,
                "G: Hypno Toad frame 1 is not deterministic across fresh instances"
            );
            check!(
                first != second,
                "G: live Expression1/clone drive did not evolve between frames"
            );
            eprintln!(
                "Plane9 Hypno Toad GPU smoke total (three draws/readbacks): {:?}",
                started.elapsed()
            );
            scene.dispose();
            repeat.dispose();
        } else {
            eprintln!("skipping Plane9 Hypno Toad GPU smoke: no fallback adapter");
        }
    }

    assert!(
        failures.is_empty(),
        "Plane9 Hypno Toad RED contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
