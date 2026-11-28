//! RED contract for `Music/Music Waves.xml`. The scene is resolved through the
//! registry and its future source/helper are read at runtime, so this compiles
//! before `Plane9 Music Waves`, `MusicWaves`, or `SoundTexture` exists.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::point10::audio_bridge::{AUDIO_SAMPLES, AudioBridge, BandPayload, RESPONSIVENESS_PROPS};
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::scenes::music_waves::{CLONE_OBJECTS_X, clone_color};
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_DUAL_TEXTURE_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_TONEMAP_WGSL,
    PLANE9_UNIFORMS_WGSL,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::test_support::{
    TestTarget, any_lit, compact, compact_contains, compact_match_count, gpu,
};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Music Waves";
const LEGACY_NAME: &str = "Point10 Music Waves";
const MUSIC_WAVES_LICENSE: &str = concat!("CC BY-", "NC-", "SA 4.0");
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

fn block_body<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
    let declaration = source.find(marker)?;
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

/// Validate a scene-owned WGSL literal with the same shared Plane9 preludes the
/// production scene composes. A literal may own its uniform/texture declarations
/// or use the shared Plane9 declarations.
fn validates_with_plane9_preludes(shader: &str) -> bool {
    let common = format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_TONEMAP_WGSL}\n");
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

fn validates_shared_fragment(fragment: &str) -> bool {
    let program =
        format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_TONEMAP_WGSL}\n{fragment}");
    let Ok(module) = naga::front::wgsl::parse_str(&program) else {
        return false;
    };
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .is_ok()
}

#[test]
fn plane9_music_waves_fidelity_red_contract() {
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

    // A — XML lines 2-6 and 255-270: one new canonical Plane9/Dahl scene.
    check!(
        scenes.len() == 52,
        "A: expected 52 Point10 scenes, got {}",
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
        names.last() == Some(&"Plane9 Cell Culture"),
        "A: Cell Culture must be the deterministic roster tail, got {:?}",
        names.last()
    );
    check!(
        names.get(names.len().saturating_sub(2)) == Some(&"Plane9 Hypno Toad"),
        "A: Hypno Toad must be penultimate, after Plane9 Music Waves"
    );
    check!(
        names.get(names.len().saturating_sub(3)) == Some(&SCENE_NAME),
        "A: Music Waves must still follow Evil Box without reordering the existing roster"
    );
    check!(
        names.get(names.len().saturating_sub(4)) == Some(&"Plane9 Evil Box"),
        "A: Music Waves must still follow Evil Box"
    );
    check!(
        PLANE9_SCENE_NAMES.len() == 34,
        "A: expected 34 Plane9 scene names, got {}",
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
        PLANE9_SCENE_NAMES.last() == Some(&"Plane9 Cell Culture"),
        "A: PLANE9_SCENE_NAMES must end with Plane9 Cell Culture"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(2))
            == Some(&"Plane9 Hypno Toad"),
        "A: PLANE9_SCENE_NAMES must place Plane9 Hypno Toad penultimate"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(3)) == Some(&SCENE_NAME),
        "A: PLANE9_SCENE_NAMES must place {SCENE_NAME} third from the end"
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

    // A — the parity fixture and generated catalogue currently pin 52/503.
    // XML line 6 is the 4.0 licence arm, not the 3.0 arm.
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("tests/fixtures/module-parity.json"))
            .expect("read module-parity.json"),
    )
    .expect("parse module-parity.json");
    check!(
        manifest["counts"]["point10"].as_u64() == Some(52),
        "A: manifest point10 count is {:?}, expected 52",
        manifest["counts"]["point10"]
    );
    check!(
        manifest["counts"]["total"].as_u64() == Some(503),
        "A: manifest total is {:?}, expected 503",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .expect("manifest point10 array")
        .iter()
        .map(|name| name.as_str().expect("manifest module name"))
        .collect();
    check!(
        manifest_names.len() == 52,
        "A: manifest point10 array has {} names, expected 52",
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
        manifest_names.last() == Some(&"Plane9 Cell Culture")
            && manifest_names.get(manifest_names.len().saturating_sub(2))
                == Some(&"Plane9 Hypno Toad")
            && manifest_names.get(manifest_names.len().saturating_sub(3)) == Some(&SCENE_NAME),
        "A: manifest must append Plane9 Cell Culture after Plane9 Hypno Toad"
    );

    let catalogue: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
            .expect("read docs/modules/modules.json"),
    )
    .expect("parse docs/modules/modules.json");
    check!(
        catalogue["counts"]["point10"].as_u64() == Some(52),
        "A: catalogue point10 count is {:?}, expected 52",
        catalogue["counts"]["point10"]
    );
    check!(
        catalogue["counts"]["total"].as_u64() == Some(503),
        "A: catalogue total is {:?}, expected 503",
        catalogue["counts"]["total"]
    );
    check!(
        catalogue["modules"].as_array().map(Vec::len) == Some(503),
        "A: catalogue modules array must contain 503 entries"
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
            .expect("catalogue Music Waves props")
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
        .filter(|line| line.starts_with("| Music Waves |"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if row.contains("music_waves.rs")
                && row.contains(MUSIC_WAVES_LICENSE)
                && row.contains("Joakim Dahl") => {}
        [row] => check!(false, "A: wrong Music Waves notices row {row:?}"),
        [] => check!(false, "A: missing Music Waves notices row"),
        _ => check!(
            false,
            "A: duplicate Music Waves notices rows {notice_rows:?}"
        ),
    }
    let ui_model = fs::read_to_string(repo_root.join("crates/modv-ui/src/model.rs"))
        .expect("read modv-ui model.rs");
    let v4_arm = ui_model
        .split("// Plane9 ports whose own headers state 4.0.")
        .nth(1)
        .and_then(|arm| arm.split("// Point10 variants").next())
        .unwrap_or_default();
    check!(
        v4_arm.contains("\"Music Waves\""),
        "A: module_license exact-4.0 arm is missing Music Waves"
    );
    check!(
        compact_contains(&ui_model, "checked.len(),52,"),
        "A: UI licence provenance count must move from 51 to 52"
    );

    // A — the app wrapper must construct the canonical name and delegate every
    // Point10Scene method plus apply_props, without this RED test naming the
    // not-yet-existing Rust type.
    let app_registry = production_source(&repo_root.join("crates/modv-app/src/module_registry.rs"))
        .expect("read app module registry");
    check!(
        compact_contains(
            &app_registry,
            "usemodv_renderers::point10::scenes::music_waves::MusicWaves;"
        ),
        "A: app registry is missing the MusicWaves import"
    );
    check!(
        compact_contains(&app_registry, "MusicWaves(MusicWaves)"),
        "A: Point10SceneKind is missing MusicWaves(MusicWaves)"
    );
    check!(
        app_registry.contains("\"Plane9 Music Waves\" => Self::MusicWaves(MusicWaves::new())"),
        "A: Point10SceneKind::create is missing Plane9 Music Waves"
    );
    check!(
        compact_match_count(&app_registry, "Self::MusicWaves(scene)") == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_match_count(&app_registry, "Self::MusicWaves(scene)")
    );

    // A/props — the vendor JS has generic clone machinery but no Music Waves
    // prop override, so the schema is exactly responsiveness + Lens.
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
    let music_waves_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = music_waves_index {
        check!(
            scenes[index].props() == expected_props,
            "A: Music Waves props must be responsiveness + Lens only; got {:?}",
            scenes[index].props()
        );
    }

    // B-I — read the not-yet-existing module at runtime. All constants and
    // shader contracts below cite the XML line(s) they transcribe.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/music_waves.rs");
    let source = production_source(&scene_path);
    if source.is_none() {
        for group in ["A", "B", "C", "D", "F", "G", "H", "I"] {
            failures.push(format!(
                "{group}: missing scene source {}",
                scene_path.display()
            ));
        }
    }
    if let Some(source) = source {
        // A — XML lines 3-6: provenance stays visible in the port's source.
        for needle in [
            "Music Waves.xml",
            "Joakim Dahl",
            "Music that creates waves",
            "Tags: `vr`",
        ] {
            check!(source.contains(needle), "A: scene source lost {needle:?}");
        }
        check!(
            source.contains("concat!(\"CC BY-\", \"NC-\", \"SA 4.0\")"),
            "A: licence marker must use the split concat! form"
        );
        // Only the machine-checked marker must be split; provenance docs may
        // legitimately spell out the full licence for readers. Pin the check to
        // the declaration itself so a contiguous rewrite cannot pass vacuously.
        let licence_declaration = source
            .lines()
            .find(|line| line.contains("const MUSIC_WAVES_LICENSE"))
            .unwrap_or_default();
        check!(
            licence_declaration.contains("concat!("),
            "A: the licence declaration must stay in the split concat! form"
        );
        check!(
            !compact_contains(licence_declaration, NC_SA_MARKER),
            "A: the licence declaration must not contain contiguous NC-SA scanner text"
        );

        // B — Screen1 (XML 10-17). P9Scene.js:560-580 applies CamRot after
        // lookAt as local-space rotateX/rotateY/rotateZ, in that order.
        for needle in [
            "CAMERA_POSITION:[f32;3]=[0.0,2.61,3.33];",
            "CAMERA_ROTATION:[f32;3]=[-31.7,0.0,0.0];",
            "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,-1.0];",
            "CAMERA_LOOK_AT_IN_WORLD_SPACE:bool=false;",
            "CAMERA_FOV:f32=45.0;",
            "CAMERA_NEAR:f32=0.1;",
            "CAMERA_FAR:f32=1000.0;",
            "SCALE_BY_ASPECT:bool=false;",
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

        // C — Disc1 (XML 120-127), CloneExpression1 (31-50), and
        // RenderObject1 (25-29). Reuse Sun's public ring builder.
        for needle in [
            "DISC_OUTER:f32=0.25;",
            "DISC_INNER:f32=0.24;",
            "DISC_SEGMENTS:u32=1000;",
            "DISC_PHASE:f32=0.0;",
            "DISC_SCALE_U:f32=1.0;",
            "DISC_SCALE_V:f32=1.0;",
            "DISC_DOUBLE_SIDED:bool=false;",
            "CLONE_OBJECTS_X:usize=100;",
            "CLONE_OBJECTS_Y:usize=1;",
            "CLONE_OBJECTS_Z:usize=1;",
            "ADJUST_FOR_ASPECT:bool=false;",
            "CLONE_CENTER:bool=false;",
            "OBJECT_POSITION:[f32;3]=[0.0,1.0,0.0];",
            "OBJECT_ROTATION:[f32;3]=[-90.0,0.0,0.0];",
            "OBJECT_SCALE:[f32;3]=[1.0,1.0,1.0];",
            "OBJECT_COLOR:[f32;4]=[1.0,1.0,1.0,1.0];",
            "sun::build_ring",
            "build_ring(DISC_INNER,DISC_OUTER,DISC_SEGMENTS,1)",
            "trs(",
        ] {
            check!(
                compact_contains(&source, needle),
                "C: lost geometry/clone constant {needle}"
            );
        }
        check!(
            !source.contains("fn build_ring"),
            "C: Music Waves must reuse sun::build_ring, not define a second ring builder"
        );
        for constant in ["DISC_PHASE", "DISC_SCALE_U", "DISC_SCALE_V"] {
            check!(
                compact_match_count(&source, constant) >= 2,
                "C: {constant} must be recorded and used, not merely documented"
            );
        }
        if let Some(body) = fn_body(&source, "clone_color") {
            for needle in ["letd=xasf32/CLONE_OBJECTS_Xasf32;", "[0.0,0.0,0.0,d]"] {
                check!(
                    compact_contains(body, needle),
                    "C: clone_color lost {needle}"
                );
            }
            for forbidden in ["in1", "out1", "perm", "rot", "scale"] {
                check!(
                    !compact_contains(body, forbidden),
                    "C: clone drive must not consume {forbidden}"
                );
            }
        } else {
            check!(false, "C: missing clone_color implementation");
        }
        // `clone_color` takes the zero-based instance index. The vendor feeds
        // x = instance + 1, so d = ((instance + 1) - 1) / 100 = instance / 100.
        check!(
            clone_color(0) == [0.0, 0.0, 0.0, 0.0],
            "C: instance 0 must have d=0.0, got {:?}",
            clone_color(0)
        );
        check!(
            clone_color(CLONE_OBJECTS_X - 1) == [0.0, 0.0, 0.0, 0.99],
            "C: instance 99 must have d=0.99 (0.9900000095367432f32), got {:?}",
            clone_color(CLONE_OBJECTS_X - 1)
        );
        let clone_colours: Vec<_> = (0..CLONE_OBJECTS_X).map(clone_color).collect();
        let alpha_range = clone_colours
            .iter()
            .map(|colour| colour[3])
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), alpha| {
                (min.min(alpha), max.max(alpha))
            });
        let rgb_stays_zero = clone_colours
            .iter()
            .all(|colour| colour[0] == 0.0 && colour[1] == 0.0 && colour[2] == 0.0);
        check!(
            rgb_stays_zero && alpha_range.0 >= 0.0 && alpha_range.1 <= 0.99,
            "C: clone d range/RGB is invalid: min={:?} max={:?} rgb_zero={}",
            alpha_range.0,
            alpha_range.1,
            rgb_stays_zero
        );
        for expression_line in [
            "d = (x-1)/numobjectsx;",
            "pos.x = 0;",
            "pos.y = 0;",
            "pos.z = 0;",
            "col.a = d;",
            "// #rot.z = noise2(0, in1)*250;",
            "// #scale.x = scale.y = scale.z = 1+d*10;",
        ] {
            check!(
                source.contains(expression_line),
                "C: clone expression lost exact line {expression_line:?}"
            );
        }
        check!(
            !compact_contains(&source, "instance_scale"),
            "C: clone scale must stay 1; no per-instance scale attribute is allowed"
        );
        check!(
            compact_contains(&source, "VertexStepMode::Instance")
                && compact_contains(&source, "VertexFormat::Float32x4")
                && compact_contains(&source, "draw_indexed(")
                && compact_contains(&source, "0..CLONE_OBJECTS_X"),
            "C: 100 rings must be instanced with a per-instance RGBA colour attribute"
        );

        // D — Shader1 (XML 68-104): angle indexes frequency columns and clone
        // alpha indexes history rows. The reversed smoothstep must stay on the
        // GLSL-compatible helper, never WGSL's undefined builtin.
        let wave = raw_const(&source, "MUSIC_WAVES_WGSL");
        check!(wave.is_some(), "D: missing MUSIC_WAVES_WGSL literal");
        if let Some(wave) = wave {
            for needle in [
                "instance_color:vec4<f32>",
                "varv=iPosition;",
                "letorgv=iPosition.xy;",
                "v.xy+=normalize(orgv.xy)*instance_color.a*2.0;",
                "atan2(orgv.y,orgv.x)/P9_PI2",
                "*0.5+0.5,instance_color.a",
                "min(0.4,",
                "p9_smoothstep(1.0,0.5,instance_color.a)",
                "v.z+=height*1.25;",
                "p9_hsv2rgb(instance_color.a*0.5,0.8,0.8)",
                "((height*10.0)+0.2)*(1.0-instance_color.a)",
                "mix(diffuse,diffuse*10.0,p9_smoothstep(0.14,0.25,height))",
                "diffuse.rgb*=0.5+p9_rand(instance_color.a)*0.5;",
                "diffuse.rgb*=0.75+p9_noise(vec3<f32>(v.xy*2.0,u.time*0.1))*0.25;",
                "letp=in.tex*2.0-1.0;",
                "returnin.diffuse;",
            ] {
                check!(
                    compact_contains(&wave, needle),
                    "D: wave shader lost {needle}"
                );
            }
            check!(
                compact_contains(&wave, "textureLoad(g_texture1,")
                    || compact_contains(&wave, "textureSampleLevel(g_texture1,g_sampler,"),
                "D: height must sample SoundTexture1"
            );
            let without_p9_smoothstep = compact(&wave).replace("p9_smoothstep(", "");
            check!(
                !compact_contains(&without_p9_smoothstep, "smoothstep("),
                "D: reversed-edge smoothstep must never use WGSL's builtin smoothstep"
            );
            check!(
                !compact_contains(&wave, "p9_light_") && !compact_contains(&wave, "lighting"),
                "D: fragment must return diffuse with no lighting pass"
            );
            for forbidden in ["in1", "out1", "perm"] {
                check!(
                    !compact_contains(&wave, forbidden),
                    "F: vestigial Expression1 term {forbidden} must not reach the wave shader"
                );
            }
            check!(
                validates_with_plane9_preludes(&wave),
                "D: wave WGSL does not parse/validate with the Plane9 helpers"
            );
        }
        for needle in [
            "SHADER1_DEPTH_TEST:bool=true;",
            "SHADER1_DEPTH_WRITE:bool=true;",
            "SHADER1_SRC_BLEND:u32=1;",
            "SHADER1_DST_BLEND:u32=0;",
            "SHADER1_CULL:u32=0;",
            "SHADER2_DEPTH_TEST:bool=false;",
            "SHADER2_DEPTH_WRITE:bool=false;",
            "SHADER2_SRC_BLEND:u32=1;",
            "SHADER2_DST_BLEND:u32=0;",
            "SHADER2_CULL:u32=2;",
            "depth_write_enabled:Some(true)",
            "depth_compare:Some(wgpu::CompareFunction::LessEqual)",
            "cull_mode:None",
            "cull_mode:Some(wgpu::Face::Back)",
            "depth_stencil:None",
            "CLEAR1:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
            "CLEAR2:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:0.0};",
        ] {
            check!(
                compact_contains(&source, needle),
                "D/I: lost render-state constant {needle}"
            );
        }

        // E — SoundTexture1 (XML 111-119). The implementation lives in a
        // reusable helper, not inline in this scene.
        for needle in [
            "SOUND_TEXTURE_SOUND_TYPE:u32=0;",
            "sound_type:SOUND_TEXTURE_SOUND_TYPE",
            "SOUND_TEXTURE_WIDTH:u32=256;",
            "SOUND_TEXTURE_HISTORY:u32=100;",
            "SOUND_TEXTURE_DAMPING:f32=0.4;",
            "SOUND_TEXTURE_INCREASE_RATE:f32=1.0;",
            "SOUND_TEXTURE_DECREASE_RATE:f32=0.5;",
            "SOUND_TEXTURE_AMPLIFICATION:f32=2.0;",
            "SoundTextureConfig",
            "SoundTexture::new",
        ] {
            check!(
                compact_contains(&source, needle),
                "E: scene lost SoundTexture1 term {needle}"
            );
        }
        check!(
            !compact_contains(&source, "fnsound_texture"),
            "E: SoundTexture must be a reusable helper, not an inline scene function"
        );
        if let Some(body) = fn_body(&source, "rebuild") {
            let sound_sampler = block_body(body, "let sound_sampler");
            check!(
                sound_sampler.is_some_and(|sampler| {
                    compact_contains(sampler, "address_mode_u:wgpu::AddressMode::Repeat")
                        && compact_contains(sampler, "address_mode_v:wgpu::AddressMode::Repeat")
                        && compact_contains(sampler, "mag_filter:wgpu::FilterMode::Linear")
                        && compact_contains(sampler, "min_filter:wgpu::FilterMode::Linear")
                        && compact_contains(
                            sampler,
                            "mipmap_filter:wgpu::MipmapFilterMode::Nearest",
                        )
                }),
                "E: SoundTexture1 needs a Repeat/Linear sampler with nearest mip filtering"
            );
            let wave_bind = block_body(body, "let wave_bind");
            check!(
                wave_bind.is_some_and(|bind| {
                    compact_contains(bind, "TextureView(sound.view())")
                        && compact_contains(bind, "Sampler(&sound_sampler)")
                }),
                "E: the wave pass must bind sound_sampler specifically to SoundTexture1"
            );
            let composite_bind = block_body(body, "let composite_bind");
            check!(
                composite_bind.is_some_and(|bind| compact_contains(bind, "Sampler(&sampler)")),
                "E: the Repeat sampler must stay scoped to SoundTexture1, not the composite"
            );
        } else {
            check!(
                false,
                "E: missing rebuild implementation for sampler binding checks"
            );
        }

        // F — Expression1 (XML 129-141) is real but vestigial: both clone
        // consumers are commented out at XML 49-50.
        check!(
            source.to_ascii_lowercase().contains("vestigial"),
            "F: the dead Expression1 drive must be documented as vestigial"
        );
        for needle in [
            "PERMRAND:f32=1.0;",
            "band(-1,1,0,0.1)",
            "*2.0+0.01",
            "permrand*10.0",
            "out1=t*0.25",
            "MusicWavesDrive",
        ] {
            check!(
                compact_contains(&source, needle),
                "F: Expression1 lost {needle}"
            );
        }
        if let Some(body) = fn_body(&source, "music_waves_drive") {
            for needle in ["(band*2.0+0.01)*dt", "permrand*10.0", "letout1=t*0.25;"] {
                check!(compact_contains(body, needle), "F: drive lost {needle}");
            }
        } else {
            check!(false, "F: missing music_waves_drive implementation");
        }

        // G — RenderToTexture1 (XML 200-242): Format 5, full scene size, one
        // mip. Its inline shader is dead because RenderObject1.Render feeds it.
        for needle in [
            "RTT_FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba16Float;",
            "RTT_MODE:u32=0;",
            "RTT_MIPS:bool=false;",
            "PREVIOUS_LAYER_MIPS:bool=false;",
            "mip_level_count:1",
        ] {
            check!(
                compact_contains(&source, needle),
                "G: RTT contract lost {needle}"
            );
        }
        check!(
            source.contains("100%") && source.contains("single-mip"),
            "G: Width/Height 0 and CreateMipMaps false must be documented"
        );
        check!(
            !compact_contains(&source, "p9_fbm") && !compact_contains(&source, "_fbm"),
            "G: dead RenderToTexture1 inline shader must not be implemented"
        );
        check!(
            !compact_contains(&source, "RTT_SIZE:u32=256")
                && !compact_contains(&source, "width:256")
                && !compact_contains(&source, "height:256"),
            "G: full-scene RTT must not use the inactive 256x256 custom size"
        );

        // H — Bloom1 (XML 243-249), mapped through UnrealBloomPass as in The
        // Cave. Intensity is construction-time, not a per-frame drive.
        for needle in [
            "BLOOM_VENDOR_RADIUS:f32=5.0;",
            "BLOOM_RADIUS:f32=0.85;",
            "BLOOM_THRESHOLD:f32=0.0;",
            "BLOOM_THRESHOLD_SMOOTHNESS:f32=0.5;",
            "BLOOM_STRENGTH:f32=0.2;",
            "BLOOM_ANTIFLICKER:bool=false;",
            "UnrealBloomPass::new(",
            "set_radius(BLOOM_RADIUS)",
            "set_threshold(BLOOM_THRESHOLD)",
        ] {
            check!(compact_contains(&source, needle), "H: Bloom1 lost {needle}");
        }
        check!(
            compact_match_count(&source, "set_strength(BLOOM_STRENGTH)") == 1,
            "H: bloom intensity 0.2 must be set exactly once at construction"
        );

        // I — Shader2 (XML 178-192): bloom gain 2, ACES over c1+c2 with alpha
        // max, then brightness/saturation/contrast, then screen blend over the
        // previous layer. The vendor composite finishes opaque.
        let composite = raw_const(&source, "MUSIC_WAVES_COMPOSITE_WGSL");
        check!(
            composite.is_some(),
            "I: missing MUSIC_WAVES_COMPOSITE_WGSL literal"
        );
        if let Some(composite) = composite {
            for needle in [
                "letp=in.tex*2.0-1.0;",
                "letc1=textureSampleLevel(g_texture1,g_sampler,tex,0.0)*2.0;",
                "letc2=textureSampleLevel(g_texture2,g_sampler,tex,0.0);",
                "letbg=textureSampleLevel(g_texture3,g_sampler,tex,0.0);",
                "letc=vec4<f32>(p9_tonemap_aces(c1.rgb+c2.rgb),max(c1.a,c2.a));",
                "varoColor=vec4<f32>(p9_saturate3(p9_brightness_saturation_contrast(c.rgb,1.0,1.5,1.0)),1.0);",
                "oColor=vec4<f32>(p9_blend_screen(oColor.rgb,bg.rgb),1.0);",
                "returnoColor;",
            ] {
                check!(
                    compact_contains(&composite, needle),
                    "I: composite shader lost {needle}"
                );
            }
            for forbidden in [
                "*0.25",
                "letc=c1+c2;",
                "p9_tonemap_aces(c.rgb)",
                "p9_blend_screen(bg.rgb,oColor.rgb)",
                "p9_blend_screen(bg.rgb,c.rgb)",
            ] {
                check!(
                    !compact_contains(&composite, forbidden),
                    "I: composite shader retained non-vendor term {forbidden}"
                );
            }
            check!(
                validates_with_plane9_preludes(&composite),
                "I: composite WGSL does not parse/validate with the Plane9 helpers"
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
                compact_contains(&source, needle),
                "I: composite binding lost {needle}"
            );
        }
        check!(
            source.contains("PreviousLayer1")
                && source.contains("placeholder")
                && source.to_ascii_lowercase().contains("transparent black"),
            "I: PreviousLayer1 must keep a transparent-black standalone placeholder"
        );
    }

    // E — reusable SoundTexture helper, pinned independently of the scene so
    // the future Spectrum scenes can share exactly the same slew/scroll rules.
    let helper_path = repo_root.join("crates/modv-renderers/src/point10/shared/sound_texture.rs");
    let helper = production_source(&helper_path);
    check!(
        helper.is_some(),
        "E: missing reusable SoundTexture helper {}",
        helper_path.display()
    );
    if let Some(helper) = helper {
        for needle in [
            "pubstructSoundTexture",
            "pubstructSoundTextureConfig",
            "pubfnupdate(",
            "if!audio.has_audio(){return;}",
            "1.0/60.0",
            "dt.clamp(0.0,0.25)",
            "width*history*4",
            "xasf32/(width-1)asf32",
            "audio.spectrum(0,p,0.0)",
            "min(1.0,dt*30.0)",
            "(1.0-damping)",
            "smoothed>=previous",
            "increase_rate",
            "decrease_rate",
            "(smoothed-previous).clamp(-step,step)",
            "value*amplification",
            ".min(1.0)",
            "value.max(0.0)",
            "(value*255.0).round()asu8",
            "row[offset]=quantized",
            "row[offset+1]=quantized",
            "row[offset+2]=quantized",
            "row[offset+3]=255",
            "copy_within(",
            "copy_from_slice(",
        ] {
            check!(
                compact_contains(&helper, needle),
                "E: SoundTexture helper lost {needle}"
            );
        }
    }
    let shared_modules =
        fs::read_to_string(repo_root.join("crates/modv-renderers/src/point10/shared/mod.rs"))
            .expect("read point10 shared module list");
    check!(
        shared_modules.contains("pub mod sound_texture;"),
        "E: point10::shared must export the reusable sound_texture module"
    );

    // I — `_blendScreen` from P9Glsl.js:408-411 belongs in the shared Plane9
    // prelude and must validate before Music Waves consumes it.
    for needle in [
        "fnp9_blend_screen(src:vec3<f32>,dest:vec3<f32>)->vec3<f32>",
        "vec3<f32>(1.0)-((vec3<f32>(1.0)-src)*(vec3<f32>(1.0)-dest))",
    ] {
        check!(
            compact_contains(PLANE9_BATCH_WGSL, needle),
            "I: shared Plane9 prelude lost {needle}"
        );
    }
    check!(
        validates_shared_fragment(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { let c = p9_blend_screen(vec3<f32>(0.2), vec3<f32>(0.4)); return vec4<f32>(c, 0.0); }"
        ),
        "I: p9_blend_screen does not parse/validate in the shared Plane9 prelude"
    );

    // J — 640x360 fallback-adapter smoke. Frame 1 and the fresh repeat use
    // dt=0 + silence for determinism. Frame 2 advances with a nonzero spectrum
    // so the new history buffer and time-driven noise must visibly evolve.
    if let Some(index) = music_waves_index {
        if let Some((device, queue)) = gpu() {
            const WIDTH: u32 = 640;
            const HEIGHT: u32 = 360;
            let started = Instant::now();
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Music Waves registry lookup");
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
                "Plane9 Music Waves GPU init (640x360, two instances): {:?}",
                started.elapsed()
            );

            let silent = BandPayload::silent(44_100.0);
            let mut evolving = BandPayload::silent(44_100.0);
            evolving.byte_frequency_data = vec![200; 1024];
            evolving.freq_mag = [0.5; AUDIO_SAMPLES];
            let mut bridge = AudioBridge::new();
            let mut repeat_bridge = AudioBridge::new();
            let target = TestTarget::new(&device, WIDTH, HEIGHT);
            let repeat_target = TestTarget::new(&device, WIDTH, HEIGHT);
            let render = |scene: &mut Box<dyn Point10Scene>,
                          bridge: &mut AudioBridge,
                          payload: &BandPayload,
                          target: &TestTarget,
                          dt: f32,
                          label: &str| {
                let frame_started = Instant::now();
                bridge.update(dt, Some(payload), &Default::default());
                scene.update(dt, bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-music-waves-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                eprintln!(
                    "Plane9 Music Waves GPU {label} (640x360): {:?}",
                    frame_started.elapsed()
                );
                pixels
            };

            let first = render(
                &mut scene,
                &mut bridge,
                &silent,
                &target,
                0.0,
                "frame 1 (dt=0, silent)",
            );
            let repeated = render(
                &mut repeat,
                &mut repeat_bridge,
                &silent,
                &repeat_target,
                0.0,
                "fresh frame 1 (dt=0, silent)",
            );
            let second = render(
                &mut scene,
                &mut bridge,
                &evolving,
                &target,
                1.0 / 60.0,
                "frame 2 (spectrum)",
            );
            check!(
                any_lit(&first),
                "J: Music Waves frame 1 is empty at 640x360"
            );
            check!(
                any_lit(&second),
                "J: Music Waves frame 2 is empty at 640x360"
            );
            check!(
                first == repeated,
                "J: Music Waves frame 1 is not deterministic across fresh instances"
            );
            check!(
                first != second,
                "J: Music Waves SoundTexture/time did not evolve between frames"
            );
            eprintln!(
                "Plane9 Music Waves GPU smoke total (three draws/readbacks): {:?}",
                started.elapsed()
            );
            scene.dispose();
            repeat.dispose();
        } else {
            eprintln!("skipping Plane9 Music Waves GPU smoke: no fallback adapter");
        }
    }

    assert!(
        failures.is_empty(),
        "Plane9 Music Waves RED contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
