//! RED contract for `Cell/Cell Culture.xml`. The scene is resolved through the
//! registry and its future source is read at runtime, so this compiles before
//! `Plane9 Cell Culture`, `CellCulture`, or any scene-owned shader exists.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::point10::audio_bridge::{AudioBridge, BandPayload, RESPONSIVENESS_PROPS};
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::scenes::plane9_common::{
    PLANE9_BATCH_WGSL, PLANE9_TEXTURE_WGSL, PLANE9_UNIFORMS_WGSL, Plane9Pass, Plane9TextureBinding,
    Plane9Uniforms,
};
use crate::point10::shared::p9_glsl::P9_COMMON_WGSL;
use crate::point10::test_support::{TestTarget, compact_contains, compact_match_count, gpu};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Cell Culture";
const LEGACY_NAME: &str = "Point10 Cell Culture";
const CELL_CULTURE_LICENSE: &str = concat!("CC BY-", "NC-", "SA 3.0");

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
///
/// Brace matching, rather than scanning for the next `fn ` token, keeps a
/// `fn ` inside a body's comment or string literal from truncating the scope.
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

/// Keep the header in the slice, including `(`, so a whole-shader needle
/// cannot be satisfied by a sibling helper's body.
fn shader_fn_scope<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    fn_scope(source, &format!("fn {name}("))
}

fn nearest_red(pixels: &[u8], width: u32, height: u32, uv: (f32, f32)) -> f32 {
    let fract = |value: f32| value - value.floor();
    let u = fract(uv.0);
    let v = fract(uv.1);
    let x = ((u * width as f32).floor() as u32).min(width - 1);
    let y = ((v * height as f32).floor() as u32).min(height - 1);
    pixels[((y * width + x) * 4) as usize] as f32 / 255.0
}

fn unorm_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn validates_seed(shader: &str) -> bool {
    let program =
        format!("{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{shader}");
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

fn validates_textured_fragment(shader: &str) -> bool {
    let program = format!(
        "{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{PLANE9_UNIFORMS_WGSL}\n{PLANE9_TEXTURE_WGSL}\n{shader}"
    );
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

fn samples_only_at_lod_zero(shader: &str) -> bool {
    let samples = compact_match_count(shader, "textureSampleLevel(");
    samples > 0
        && samples == compact_match_count(shader, ",0.0)")
        && compact_match_count(shader, "textureSample(") == 0
        && compact_match_count(shader, "texture(") == 0
}

/// The presented seed is the binary reaction field multiplied by Color1
/// `(0.8, 0.9, 0.9, 1)`. Cell Culture's seed broadcasts `s` into all four
/// channels, so presentation is `(0.8*s, 0.9*s, 0.9*s, s)`, not opaque RGB.
/// Quantisation permits one 8-bit step of slack around 0 and
/// `(0.8*255, 0.9*255, 0.9*255, 255)`.
fn presented_seed_is_binary(pixels: &[u8]) -> bool {
    let mut has_zero = false;
    let mut has_one = false;
    for pixel in pixels.as_chunks::<4>().0 {
        let is_zero = pixel.iter().all(|channel| *channel <= 1);
        let is_one = (pixel[0] as f32 - 0.8 * 255.0).abs() <= 1.0
            && (pixel[1] as f32 - 0.9 * 255.0).abs() <= 1.0
            && (pixel[2] as f32 - 0.9 * 255.0).abs() <= 1.0
            && (pixel[3] as f32 - 255.0).abs() <= 1.0;
        if !is_zero && !is_one {
            return false;
        }
        has_zero |= is_zero;
        has_one |= is_one;
    }
    has_zero && has_one
}

/// `vec4<f32>(step(vc, 0.5))` is a scalar broadcast in WGSL: all four
/// channels, including alpha, receive the same `s`. Accept either a direct
/// broadcast of `step(vc, 0.5)` or an explicitly named `s` returned in every
/// component. The caller separately rejects the non-vendor RGB-plus-opaque
/// alpha shapes.
fn seed_returns_same_step_in_all_channels(seed: &str) -> bool {
    let Some(body) = fn_body(seed, "fs_main") else {
        return false;
    };
    let step_is_named = compact_contains(body, "let s = step(vc, 0.5);");
    let named_return = compact_contains(body, "return vec4<f32>(s, s, s, s);")
        || compact_contains(body, "return vec4<f32>(vec4<f32>(s));");
    let direct_return = compact_contains(body, "return vec4<f32>(step(vc, 0.5));")
        || compact_contains(body, "return vec4<f32>(vec4<f32>(step(vc, 0.5)));");
    (step_is_named && named_return) || direct_return
}

#[test]
fn plane9_cell_culture_fidelity_red_contract() {
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

    // A — XML 2-6 and the registry contract: the eighth XML batch appends one
    // canonical Plane9/Dahl scene after Hypno Toad.
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
        names.get(names.len().saturating_sub(2)) == Some(&"Plane9 Waves of Sin"),
        "A: Waves of Sin must follow Plane9 Cell Culture without reordering the roster"
    );
    check!(
        names.get(names.len().saturating_sub(3)) == Some(&SCENE_NAME),
        "A: Cell Culture must follow Plane9 Hypno Toad without reordering the roster"
    );
    check!(
        names.get(names.len().saturating_sub(4)) == Some(&"Plane9 Hypno Toad"),
        "A: Cell Culture must follow Plane9 Hypno Toad without reordering the roster"
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
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(2))
            == Some(&"Plane9 Waves of Sin"),
        "A: PLANE9_SCENE_NAMES must place Waves of Sin penultimate"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(3)) == Some(&SCENE_NAME),
        "A: PLANE9_SCENE_NAMES must append Cell Culture after Hypno Toad"
    );
    check!(
        PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(4))
            == Some(&"Plane9 Hypno Toad"),
        "A: PLANE9_SCENE_NAMES must place Hypno Toad before Cell Culture"
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

    // A — the parity fixture and generated catalogue move from 51/502 to
    // 54/506. XML line 6 is the 3.0 licence arm, not the 4.0 arm.
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
        manifest["counts"]["total"].as_u64() == Some(506),
        "A: manifest total is {:?}, expected 506",
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
            && manifest_names.get(manifest_names.len().saturating_sub(2))
                == Some(&"Plane9 Waves of Sin")
            && manifest_names.get(manifest_names.len().saturating_sub(3)) == Some(&SCENE_NAME)
            && manifest_names.get(manifest_names.len().saturating_sub(4))
                == Some(&"Plane9 Hypno Toad"),
        "A: manifest must append Plane9 Waves of Sin after {SCENE_NAME}"
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
        catalogue["counts"]["total"].as_u64() == Some(506),
        "A: catalogue total is {:?}, expected 506",
        catalogue["counts"]["total"]
    );
    check!(
        catalogue["modules"].as_array().map(Vec::len) == Some(506),
        "A: catalogue modules array must contain 506 entries"
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
            "A: empty XML Desc must use the sibling null-description convention, got {:?}",
            module["description"]
        );
        let prop_names: Vec<&str> = module["props"]
            .as_array()
            .expect("catalogue Cell Culture props")
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
        .filter(|line| line.starts_with("| Cell Culture |"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if compact_contains(row, "cell_culture.rs")
                && compact_contains(row, CELL_CULTURE_LICENSE)
                && compact_contains(row, "Joakim Dahl") => {}
        [row] => check!(false, "A: wrong Cell Culture notices row {row:?}"),
        [] => check!(false, "A: missing Cell Culture notices row"),
        _ => check!(
            false,
            "A: duplicate Cell Culture notices rows {notice_rows:?}"
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
        compact_contains(v3_arm, "\"Cell Culture\""),
        "A: module_license exact-3.0 arm is missing Cell Culture"
    );
    check!(
        !compact_contains(v4_arm, "\"Cell Culture\""),
        "A: Cell Culture must not be classified in the 4.0 licence arm"
    );
    check!(
        compact_contains(&ui_model, "\"Point10 Cell Culture\""),
        "A: UI provenance test is missing Point10 Cell Culture"
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
            "usemodv_renderers::point10::scenes::cell_culture::CellCulture;"
        ),
        "A: app registry is missing the CellCulture import"
    );
    check!(
        compact_contains(&app_registry, "CellCulture(CellCulture)"),
        "A: Point10SceneKind is missing CellCulture(CellCulture)"
    );
    check!(
        compact_contains(
            &app_registry,
            "\"Plane9 Cell Culture\"=>Self::CellCulture(CellCulture::new())"
        ),
        "A: Point10SceneKind::create is missing Plane9 Cell Culture"
    );
    check!(
        compact_match_count(&app_registry, "Self::CellCulture(scene)") == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_match_count(&app_registry, "Self::CellCulture(scene)")
    );
    let app_registry_with_tests =
        fs::read_to_string(repo_root.join("crates/modv-app/src/module_registry.rs"))
            .expect("read app module registry with tests");
    let wrapper_order = fn_body(
        &app_registry_with_tests,
        "point10_definitions_match_the_wrapper_modules",
    )
    .unwrap_or_default();
    let hypno_position = wrapper_order.find("\"Plane9 Hypno Toad\"");
    let cell_position = wrapper_order.find("\"Plane9 Cell Culture\"");
    let waves_position = wrapper_order.find("\"Plane9 Waves of Sin\"");
    let gravity_position = wrapper_order.find("\"Plane9 Gravity Spectrum\"");
    check!(
        hypno_position.is_some_and(|hypno| { cell_position.is_some_and(|cell| hypno < cell) }),
        "A: wrapper-order list must append Plane9 Cell Culture after Plane9 Hypno Toad"
    );
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

    // A/props — the vendor JS has no sceneProps.js and no Cell Culture prop
    // override, so the schema is exactly responsiveness + Lens.
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
    let cell_culture_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = cell_culture_index {
        check!(
            scenes[index].props() == expected_props,
            "A: Cell Culture props must be responsiveness + Lens only; got {:?}",
            scenes[index].props()
        );
    }

    // B-G — read the not-yet-existing module at runtime. All constants and
    // shader contracts below cite the XML/JS lines they transcribe.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/cell_culture.rs");
    let source = production_source(&scene_path);
    let evolve = source
        .as_deref()
        .and_then(|source| raw_const(source, "EVOLVE"));
    if source.is_none() {
        for group in ["B", "C", "D", "E", "F"] {
            failures.push(format!(
                "{group}: missing scene source {}",
                scene_path.display()
            ));
        }
    }
    if let Some(source) = source.as_deref() {
        let code = rust_code(source);
        let header = source
            .split("#![forbid(unsafe_code)]")
            .next()
            .unwrap_or(source);

        // A — XML 3-6: provenance stays visible. Empty Desc follows Abstract
        // Clouds/Magic Smoke Box: record the tags, invent no description.
        for needle in ["Cell Culture.xml", "Joakim Dahl", "Tags: `mellow`"] {
            check!(
                compact_contains(source, needle),
                "A: scene source lost {needle:?}"
            );
        }
        check!(
            compact_contains(source, CELL_CULTURE_LICENSE),
            "A: scene source must state the exact 3.0 licence"
        );
        check!(
            !compact_contains(header, "description is")
                && !compact_contains(header, "own description"),
            "A: empty XML Desc must not acquire an invented description"
        );

        // B — Screen1 (XML 8-17), both RenderRects (22-25, 114-117), and both
        // Clear nodes (19-21, 238-240). CamLookAt is relative in P9Scene.js:573.
        for needle in [
            "CAMERA_POSITION:[f32;3]=[0.0,0.0,-2.0];",
            "CAMERA_ROTATION:[f32;3]=[0.0,0.0,0.0];",
            "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,1.0];",
            "CAMERA_LOOK_AT_IN_WORLD_SPACE:bool=false;",
            "CAMERA_FOV:f32=45.0;",
            "CAMERA_NEAR:f32=0.1;",
            "CAMERA_FAR:f32=1000.0;",
            "SCALE_BY_ASPECT:bool=false;",
            "RENDER_RECT1_TESSELATE_W:u32=1;",
            "RENDER_RECT1_TESSELATE_H:u32=1;",
            "RENDER_RECT2_TESSELATE_W:u32=1;",
            "RENDER_RECT2_TESSELATE_H:u32=1;",
            "CLEAR1:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
            "CLEAR2:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
        ] {
            check!(
                compact_contains(source, needle),
                "B: lost camera/rect/clear constant {needle}"
            );
        }
        check!(
            compact_contains(source, "relative")
                && compact_contains(source, "added to the position"),
            "B: CamLookAt (0,0,1) must be documented as relative to CamPos"
        );
        check!(
            compact_contains(source, "plane9_scene_props(&[])")
                && compact_contains(&code, "lens_value(")
                && compact_contains(&code, "apply_lens("),
            "A/B: Cell Culture must use the standard no-override props and Lens helpers"
        );

        // C — RenderToTexture1 (XML 71-113) and StoreTexture1 (241-254).
        // Reuse the existing ping-pong harness; only this scene's taps/constants
        // belong here. Format 1 follows Abstract Clouds' Rgba8Unorm precedent.
        for needle in [
            "RTT_FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba8Unorm;",
            "RTT_MODE:u32=0;",
            "RTT_MIPS:bool=true;",
            "SWAP_DELAY:f32=0.0;",
            "feedback:Plane9Feedback",
            "Plane9Feedback::new(",
            "plane9_sampler(",
            "seed:Plane9Pass",
            "evolve:Plane9Pass",
            "present:Plane9Pass",
            "100%",
            "single-mip",
            "recorded",
            "inert",
        ] {
            check!(
                compact_contains(source, needle),
                "C: lost feedback/RTT term {needle}"
            );
        }
        check!(
            !compact_contains(&code, "structPlane9Feedback")
                && !compact_contains(&code, "structFeedback"),
            "C: Cell Culture must reuse Plane9Feedback, not define another feedback harness"
        );
        check!(
            compact_contains(&code, "previous_view()")
                && compact_contains(&code, "current_view()")
                && compact_contains(&code, ".swap()"),
            "C: feedback must read previous_view, write current_view, then swap"
        );
        if let Some(body) = fn_body(&code, "render") {
            let previous = body.find("previous_view()");
            let current = body.find("current_view()");
            let swap = body.find(".swap()");
            check!(
                previous.is_some_and(|previous| {
                    current.is_some_and(|current| {
                        swap.is_some_and(|swap| previous < current && current < swap)
                    })
                }),
                "C: render must sample previous, draw current, present current, then swap"
            );
            check!(
                compact_contains(body, "ifself.frame<SEED_FRAMES")
                    && compact_contains(body, "gpu.seed.draw(")
                    && compact_contains(body, "gpu.evolve")
                    && compact_contains(body, "gpu.present"),
                "C: gFrameNr < 2 must be a dedicated seed pass, not an evolve-shader branch"
            );
        } else {
            check!(false, "C: missing render implementation");
        }
        for forbidden in [
            "generate_mipmaps",
            "MipTarget",
            "mip_pipeline",
            "mip_binds",
            "fs_mip",
            "p9_fbm",
            "_fbm",
        ] {
            check!(
                !compact_contains(source, forbidden),
                "C: dead/inert vendor feature must not be implemented: {forbidden}"
            );
        }
        check!(
            compact_contains(&code, "SEED_FRAMES:u32=2;"),
            "C: gFrameNr < 2 must record SEED_FRAMES = 2"
        );

        // D — Shader1 (XML 118-237). Every vendor term is pinned, including the
        // a4/v4 radius asymmetry: a4 averages radius 16, v4 varies radius 10.
        for needle in [
            "RADII:[f32;4]=[2.0,4.0,8.0,16.0];",
            "VARIANCE_RADII:[f32;4]=[2.0,4.0,8.0,10.0];",
            "THRESHOLDS:[f32;4]=[0.50,0.55,0.60,0.65];",
            "VARIANCE_WEIGHT:f32=0.2;",
            "CHANGE_STEP:f32=0.005;",
            "RETENTION:f32=0.79;",
            "SHADER1_DEPTH_TEST:bool=true;",
            "SHADER1_DEPTH_WRITE:bool=true;",
            "SHADER1_SRC_BLEND:u32=1;",
            "SHADER1_DST_BLEND:u32=0;",
            "SHADER1_CULL:u32=2;",
        ] {
            check!(
                compact_contains(source, needle),
                "D: lost reaction constant/state {needle}"
            );
        }
        let seed = raw_const(source, "SEED");
        check!(seed.is_some(), "D: missing SEED literal");
        if let Some(seed) = seed.as_deref() {
            for needle in [
                "lettex=position.xy/u.resolution;",
                "letp=tex*vec2<f32>(2.0)-vec2<f32>(1.0);",
                "letvc=p9_perm4_vec2(p*vec2<f32>(0.15)+u.in3.xx).r;",
            ] {
                check!(compact_contains(seed, needle), "D: seed lost {needle}");
            }
            check!(
                seed_returns_same_step_in_all_channels(seed),
                "D: seed fs_main must return the same step(vc, 0.5) value in R/G/B/A \
                 (vec4<f32>(s, s, s, s), its scalar broadcast, or a direct step broadcast)"
            );
            for forbidden in [
                "return vec4<f32>(s, 1.0, 1.0, 1.0);",
                "return vec4<f32>(s, 1, 1, 1);",
                "return vec4<f32>(field.r, 1.0, 1.0, 1.0);",
                "return vec4<f32>(field.r, 1, 1, 1);",
            ] {
                check!(
                    !compact_contains(seed, forbidden),
                    "D: seed must not replace the vendor broadcast alpha with the non-vendor shape {forbidden}"
                );
            }
            check!(
                !compact_contains(seed, "u.time")
                    && !compact_contains(seed, "g_texture1")
                    && compact_match_count(seed, "gFrameNr") == 0,
                "D: seed must stay time-independent and texture-free"
            );
            check!(
                validates_seed(seed),
                "D: SEED WGSL does not parse/validate with the shared Plane9 preludes"
            );
        }
        check!(evolve.is_some(), "D: missing EVOLVE literal");
        if let Some(evolve) = evolve.as_deref() {
            for needle in [
                "structP9EvalVar",
                "value:f32",
                "change:f32",
                "fnp9_eval_avg(",
                "fnp9_eval_var(",
                "for(vardir_nr=0;dir_nr<9;dir_nr=dir_nr+1)",
                "f32(dir_nr)*P9_PI2/9.0+u.in2.z",
                "radius*cos(dir)",
                "radius*sin(dir)",
                "fract(p+tp*d)",
                "letp=tex*vec2<f32>(2.0)-vec2<f32>(1.0);",
                "lettp=vec2<f32>(1.0)/u.resolution;",
                "letc=textureSampleLevel(g_texture1,g_sampler,tex,0.0).r;",
                "leta1=p9_eval_avg(tex,tp,2.0);",
                "leta2=p9_eval_avg(tex,tp,4.0);",
                "leta3=p9_eval_avg(tex,tp,8.0);",
                "leta4=p9_eval_avg(tex,tp,16.0);",
                "letv1=p9_eval_var(tex,tp,2.0,a1,0.5);",
                "letv2=p9_eval_var(tex,tp,4.0,a2,0.55);",
                "letv3=p9_eval_var(tex,tp,8.0,a3,0.6);",
                "letv4=p9_eval_var(tex,tp,10.0,a4,0.65);",
                "letnext=((v1.value+v2.value+v3.value+v4.value)*0.2+(v1.change+v2.change+v3.change+v4.change)+c)*0.79;",
                "returnvec4<f32>(vec3<f32>(next),1.0);",
            ] {
                check!(compact_contains(evolve, needle), "D: evolve lost {needle}");
            }

            // XML 173-191 normalises EvalVar exactly once, on return. Scope
            // these pins to each helper so EvalAvg's `return co / 9.0` cannot
            // satisfy EvalVar's return assertion.
            match shader_fn_scope(evolve, "p9_eval_avg") {
                Some(avg) => {
                    check!(
                        compact_contains(
                            avg,
                            "co = co + textureSampleLevel(g_texture1, g_sampler, \
                             fract(p + tp * d), 0.0).r;"
                        ),
                        "D/p9_eval_avg: accumulation must sum the nine unnormalised source taps"
                    );
                    check!(
                        compact_contains(avg, "return co / 9.0;"),
                        "D/p9_eval_avg: the nine-tap sum must be divided by 9 only on return"
                    );
                }
                None => check!(false, "D/p9_eval_avg: missing function scope"),
            }
            match shader_fn_scope(evolve, "p9_eval_var") {
                Some(variance) => {
                    check!(
                        compact_contains(variance, "co = co + abs(src - avg);"),
                        "D/p9_eval_var: accumulation must be unnormalised; XML 185 adds \
                         abs(src - avg) and XML 191 divides once on return"
                    );
                    check!(
                        !compact_contains(variance, "abs(src - avg) / 9.0"),
                        "D/p9_eval_var: the loop must not divide by 9; combining that with \
                         the return division would produce the non-vendor co / 81"
                    );
                    check!(
                        compact_contains(variance, "return P9EvalVar(co / 9.0, change);"),
                        "D/p9_eval_var: return must be P9EvalVar(co / 9.0, change)"
                    );
                    check!(
                        compact_contains(variance, "change = change + 0.005;")
                            && compact_contains(variance, "change = change - 0.005;"),
                        "D/p9_eval_var: per-tap change accumulation must remain +0.005/-0.005"
                    );
                }
                None => check!(false, "D/p9_eval_var: missing function scope"),
            }
            for forbidden in [
                "*0.53",
                "*0.002",
                "*0.85",
                "p9_eval_var(tex,tp,16.0,a4",
                "change=change+1.0",
                "change=change-1.0",
            ] {
                check!(
                    !compact_contains(evolve, forbidden),
                    "D: evolve retained a sibling/non-vendor term {forbidden}"
                );
            }
            check!(
                samples_only_at_lod_zero(evolve),
                "D: every reaction texture sample must use explicit LOD 0"
            );
            check!(
                validates_textured_fragment(evolve),
                "D: EVOLVE WGSL does not parse/validate with the shared Plane9 preludes"
            );
        }

        // E — Shader2 (XML 26-70): texture * diffuse, with the vertex diffuse
        // carrying Color1 (0.8, 0.9, 0.9, 1). The shared full-screen pass has
        // white iColor/gColor, so u.color1 is the equivalent portable binding.
        // SrcBlend 1 / DstBlend 0 is One/Zero (replace), not an opaque override:
        // alpha is sampled.a * color1.a. We deliberately pin Cell Culture's
        // vendor formula rather than the sibling opacity conventions asserted
        // below; seed alpha therefore remains s, while EVOLVE stores alpha 1.
        for needle in [
            "TINT:[f32;4]=[0.8,0.9,0.9,1.0];",
            "SHADER2_DEPTH_TEST:bool=true;",
            "SHADER2_DEPTH_WRITE:bool=true;",
            "SHADER2_SRC_BLEND:u32=1;",
            "SHADER2_DST_BLEND:u32=0;",
            "SHADER2_CULL:u32=2;",
            "so.diffuse=iColor*gColor1*gColor",
        ] {
            check!(
                compact_contains(source, needle),
                "E: lost tint/state/vertex term {needle}"
            );
        }
        check!(
            compact_match_count(source, "TINT") >= 2
                && compact_contains(&code, "self.uniforms.color1=TINT"),
            "E: the 0.8/0.9/0.9 tint must be recorded and uploaded to the present pass"
        );
        let present = raw_const(source, "PRESENT");
        check!(present.is_some(), "E: missing PRESENT literal");
        if let Some(present) = present.as_deref() {
            check!(
                compact_contains(
                    present,
                    "textureSampleLevel(g_texture1,g_sampler,tex,0.0)*u.color1"
                ),
                "E: present must be texture(gTexture1, si.tex) * diffuse"
            );
            check!(
                compact_contains(present, "return tinted;")
                    || compact_contains(
                        present,
                        "return textureSampleLevel(g_texture1,g_sampler,tex,0.0)*u.color1;",
                    ),
                "E: present must return the complete sampled * tinted vector, including sampled.a * tint.a"
            );
            check!(
                !compact_contains(present, "1.0"),
                "E: present must not force alpha 1.0; One/Zero replace preserves sampled.a * tint.a"
            );
            check!(
                samples_only_at_lod_zero(present),
                "E: every composite texture sample must use explicit LOD 0"
            );
            check!(
                validates_textured_fragment(present),
                "E: PRESENT WGSL does not parse/validate with the shared Plane9 preludes"
            );

            // Explicit sibling deviations: Abstract Clouds' PRESENT hard-codes
            // alpha 1.0, and Magic Smoke Box stores alpha 1.0 in both SEED and
            // EVOLVE. Those are not Cell Culture's XML semantics and must not be
            // copied into its sampled-alpha composite.
            let abstract_clouds = production_source(
                &repo_root.join("crates/modv-renderers/src/point10/scenes/abstract_clouds.rs"),
            )
            .expect("read Abstract Clouds source");
            let abstract_clouds_present = raw_const(&abstract_clouds, "PRESENT")
                .expect("read Abstract Clouds PRESENT literal");
            check!(
                compact_contains(
                    &abstract_clouds_present,
                    "var out = vec4<f32>(c, c * 0.25 + 0.75, 0.95, 1.0);",
                ),
                "E: documented Abstract Clouds deviation (PRESENT forces alpha 1.0) changed"
            );
            let magic_smoke_box = production_source(
                &repo_root.join("crates/modv-renderers/src/point10/scenes/magic_smoke_box.rs"),
            )
            .expect("read Magic Smoke Box source");
            let magic_smoke_box_seed =
                raw_const(&magic_smoke_box, "SEED").expect("read Magic Smoke Box SEED literal");
            let magic_smoke_box_evolve =
                raw_const(&magic_smoke_box, "EVOLVE").expect("read Magic Smoke Box EVOLVE literal");
            check!(
                compact_contains(&magic_smoke_box_seed, "p9_perm4_vec2(p + u.in3.zz).r, 1.0,",)
                    && compact_contains(
                        &magic_smoke_box_evolve,
                        "return vec4<f32>(next.rgb, 1.0);",
                    ),
                "E: documented Magic Smoke Box deviation (SEED/EVOLVE store alpha 1.0) changed"
            );
        }
        check!(
            (compact_contains(source, "full-screen") || compact_contains(source, "fullscreen"))
                && compact_contains(source, "cull")
                && compact_contains(source, "inert"),
            "D/E: depth/cull state must be recorded and the fullscreen cull mode documented as inert"
        );

        // F — Expression1 (XML 326-333). gIn2.z rotates all rings; gIn3.x seeds
        // _perm. in1.x is evaluated but unread by every shader literal.
        for needle in [
            "PERMRAND:f32=1.0;",
            "in1.x=time;",
            "in2.x=random()*100;",
            "in2.y=random()*100;",
            "in2.z=random()*PI*2;",
            "in3.x=permrand;",
            "unread",
        ] {
            check!(
                compact_contains(source, needle),
                "F: expression/provenance lost {needle}"
            );
        }
        if let Some(body) = fn_body(&code, "update") {
            for needle in [
                "letframe=self.frameasi32;",
                "letrandom=|salt:i32|seeded_rand(frame,salt);",
                "self.uniforms.time=self.elapsed;",
                "random(101)*100.0",
                "random(103)*100.0",
                "random(107)*std::f32::consts::TAU",
                "self.uniforms.in3=[self.permrand,0.0,0.0,0.0];",
            ] {
                check!(compact_contains(body, needle), "F: update lost {needle}");
            }
            check!(
                compact_contains(body, "self.uniforms.in1[0]=self.elapsed")
                    || compact_contains(body, "self.uniforms.in1=[self.elapsed,0.0,0.0,0.0]"),
                "F: in1.x must be set to elapsed time even though the shader does not read it"
            );
        } else {
            check!(false, "F: missing update implementation");
        }
        for shader in [seed.as_deref(), evolve.as_deref(), present.as_deref()]
            .into_iter()
            .flatten()
        {
            check!(
                !compact_contains(shader, "u.in1") && !compact_contains(shader, "gIn1"),
                "F: in1.x is vestigial and must stay unread by the shaders"
            );
        }
    }

    // G — 320x180 fallback-adapter smoke. Three frames cover both dedicated
    // seed frames and one evolve frame. The vendor performs 73 samples per
    // evolved pixel (4*9 average + 4*9 variance + centre), so keep the target
    // small and print each draw/readback timing.
    if let Some(index) = cell_culture_index {
        check!(
            source.is_some(),
            "G: registered Cell Culture has no scene source {}",
            scene_path.display()
        );
        if let Some((device, queue)) = gpu() {
            // A source-shape pin alone previously let the double-normalised
            // variance through. Replant a deterministic high-contrast field as
            // gTexture1, render the production EvalAvg/EvalVar helpers once,
            // and compare the red output with both CPU analytics: sum/9 and
            // the non-vendor sum/81. Nearest sampling makes the nine taps exact.
            if let Some(evolve) = evolve.as_deref() {
                const PROBE_WIDTH: u32 = 65;
                const PROBE_HEIGHT: u32 = 65;
                const PROBE_RADIUS: f32 = 10.0;
                let helpers = evolve.split("@fragment").next().unwrap_or(evolve);
                let probe = format!(
                    "{P9_COMMON_WGSL}\n{PLANE9_BATCH_WGSL}\n{helpers}\n{}",
                    r#"
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let tex = position.xy / u.resolution;
  let tp = vec2<f32>(1.0) / u.resolution;
  let avg = p9_eval_avg(tex, tp, 10.0);
  let result = p9_eval_var(tex, tp, 10.0, avg, 0.65);
  return vec4<f32>(result.value, result.change, avg, 1.0);
}
"#
                );

                let row_bytes = PROBE_WIDTH * 4;
                let mut input_pixels = Vec::with_capacity((row_bytes * PROBE_HEIGHT) as usize);
                for y in 0..PROBE_HEIGHT {
                    for x in 0..PROBE_WIDTH {
                        let red = if ((x / 4) + (y / 4)) % 2 == 0 { 0 } else { 255 };
                        input_pixels.extend_from_slice(&[red, red, red, 255]);
                    }
                }
                let padded_row = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                    * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let mut upload = vec![0; (padded_row * PROBE_HEIGHT) as usize];
                for y in 0..PROBE_HEIGHT {
                    let source = (y * row_bytes) as usize;
                    let destination = (y * padded_row) as usize;
                    upload[destination..destination + row_bytes as usize]
                        .copy_from_slice(&input_pixels[source..source + row_bytes as usize]);
                }
                let feedback_input = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("point10-cell-culture-variance-feedback-input"),
                    size: wgpu::Extent3d {
                        width: PROBE_WIDTH,
                        height: PROBE_HEIGHT,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    feedback_input.as_image_copy(),
                    &upload,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_row),
                        rows_per_image: Some(PROBE_HEIGHT),
                    },
                    wgpu::Extent3d {
                        width: PROBE_WIDTH,
                        height: PROBE_HEIGHT,
                        depth_or_array_layers: 1,
                    },
                );
                let feedback_view =
                    feedback_input.create_view(&wgpu::TextureViewDescriptor::default());
                let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("point10-cell-culture-variance-nearest-sampler"),
                    address_mode_u: wgpu::AddressMode::ClampToEdge,
                    address_mode_v: wgpu::AddressMode::ClampToEdge,
                    address_mode_w: wgpu::AddressMode::ClampToEdge,
                    mag_filter: wgpu::FilterMode::Nearest,
                    min_filter: wgpu::FilterMode::Nearest,
                    mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                    ..Default::default()
                });
                let uniforms = Plane9Uniforms::new(PROBE_WIDTH, PROBE_HEIGHT);
                let probe_pass = Plane9Pass::new(
                    &device,
                    &queue,
                    wgpu::TextureFormat::Rgba8Unorm,
                    "point10-cell-culture-variance-probe",
                    &probe,
                    Some(Plane9TextureBinding::D2 {
                        view: &feedback_view,
                        sampler: &sampler,
                    }),
                );
                probe_pass.upload(&uniforms);
                let probe_target = TestTarget::new(&device, PROBE_WIDTH, PROBE_HEIGHT);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-cell-culture-variance-probe"),
                });
                probe_pass.draw(
                    &mut encoder,
                    &probe_target.view,
                    wgpu::Color::BLACK,
                    "point10-cell-culture-variance-probe",
                );
                queue.submit(Some(encoder.finish()));
                let actual = probe_target.read(&device, &queue);

                let pixel_count = (PROBE_WIDTH * PROBE_HEIGHT) as usize;
                let mut vendor_mismatches = 0usize;
                let mut max_vendor_delta = 0u32;
                let mut actual_small_delta_sum = 0u32;
                let mut analytic_delta_sum = 0u32;
                for y in 0..PROBE_HEIGHT {
                    for x in 0..PROBE_WIDTH {
                        let center = (
                            (x as f32 + 0.5) / PROBE_WIDTH as f32,
                            (y as f32 + 0.5) / PROBE_HEIGHT as f32,
                        );
                        let tp = (1.0 / PROBE_WIDTH as f32, 1.0 / PROBE_HEIGHT as f32);
                        let mut taps = [0.0f32; 9];
                        for (dir_nr, tap) in taps.iter_mut().enumerate() {
                            let angle = dir_nr as f32 * std::f32::consts::TAU / 9.0;
                            let dx = PROBE_RADIUS * angle.cos();
                            let dy = PROBE_RADIUS * angle.sin();
                            let uv = (center.0 + tp.0 * dx, center.1 + tp.1 * dy);
                            *tap = nearest_red(&input_pixels, PROBE_WIDTH, PROBE_HEIGHT, uv);
                        }
                        let avg = taps.iter().sum::<f32>() / 9.0;
                        let deviation_sum = taps.iter().map(|src| (*src - avg).abs()).sum::<f32>();
                        let vendor = unorm_byte(deviation_sum / 9.0);
                        let small = unorm_byte(deviation_sum / 81.0);
                        let actual_red = actual[((y * PROBE_WIDTH + x) * 4) as usize];
                        let vendor_delta = actual_red.abs_diff(vendor) as u32;
                        vendor_mismatches += usize::from(vendor_delta > 1);
                        max_vendor_delta = max_vendor_delta.max(vendor_delta);
                        actual_small_delta_sum += actual_red.abs_diff(small) as u32;
                        analytic_delta_sum += vendor.abs_diff(small) as u32;
                    }
                }
                let analytic_gap = analytic_delta_sum as f32 / pixel_count as f32;
                let actual_small_gap = actual_small_delta_sum as f32 / pixel_count as f32;
                check!(
                    analytic_gap > 1.0 && vendor_mismatches == 0 && actual_small_gap > 1.0,
                    "D/numeric: planted nine-tap variance probe must render sum/9, not sum/81; \
                     analytic_gap={analytic_gap:.3}, vendor_mismatches={vendor_mismatches}, \
                     max_vendor_delta={max_vendor_delta}, actual_vs_small_mean={actual_small_gap:.3}"
                );
            }

            const WIDTH: u32 = 320;
            const HEIGHT: u32 = 180;
            let started = Instant::now();
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Cell Culture registry lookup");
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
                "Plane9 Cell Culture GPU init (320x180, two instances): {:?}",
                started.elapsed()
            );

            let silent = BandPayload::silent(44_100.0);
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
                bridge.update(dt, Some(&silent), &Default::default());
                scene.update(dt, bridge);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("point10-plane9-cell-culture-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                eprintln!(
                    "Plane9 Cell Culture GPU {label} (320x180): {:?}",
                    frame_started.elapsed()
                );
                pixels
            };

            let mut frames = Vec::new();
            let mut repeat_frames = Vec::new();
            for frame in 0..3 {
                frames.push(render(
                    &mut scene,
                    &mut bridge,
                    &target,
                    0.0,
                    &format!("frame {frame} (dt=0, silent)"),
                ));
                repeat_frames.push(render(
                    &mut repeat,
                    &mut repeat_bridge,
                    &repeat_target,
                    0.0,
                    &format!("fresh frame {frame} (dt=0, silent)"),
                ));
            }
            check!(
                presented_seed_is_binary(&frames[0]) && presented_seed_is_binary(&frames[1]),
                "G: seed frames must contain only (0,0,0,0) and (0.8,0.9,0.9,1) \
                 within one 8-bit step, with both field states present"
            );
            check!(
                frames[0] == frames[1],
                "G: the two dedicated seed frames must be byte-identical"
            );
            check!(
                frames[2] != frames[1] && frames[2] != frames[0],
                "G: the evolved frame must differ from the seed field"
            );
            check!(
                frames[2]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] == 255),
                "G: the evolved frame's stored alpha must remain 1.0 through the sampled-alpha composite"
            );
            for frame in 0..3 {
                check!(
                    frames[frame] == repeat_frames[frame],
                    "G: fresh instances diverged on frame {frame} with dt=0"
                );
            }
            eprintln!(
                "Plane9 Cell Culture GPU smoke total (six draws/readbacks): {:?}",
                started.elapsed()
            );
            scene.dispose();
            repeat.dispose();
        } else {
            eprintln!("skipping Plane9 Cell Culture GPU smoke: no fallback adapter");
        }
    } else {
        failures.push(format!(
            "G: missing registered scene {SCENE_NAME}; GPU smoke cannot run"
        ));
    }

    assert!(
        failures.is_empty(),
        "Plane9 Cell Culture RED contract failed ({} checks):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
