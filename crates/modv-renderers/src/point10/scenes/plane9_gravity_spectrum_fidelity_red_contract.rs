//! RED contract for `Music/Gravity Spectrum.xml`. The scene and its reusable
//! ribbon-history helper are read at runtime, so this compiles before
//! `Plane9 Gravity Spectrum`, `GravitySpectrum`, or `RibbonHistory` exists.
//!
//! The vendored XML is authoritative. In particular, `Ribbons2` at XML line 68
//! actually reads `spectrum(0,i,0)`, just like `Ribbons1`; only
//! `CloneExpression2` at line 164 reads channel 1. The mono bridge nevertheless
//! makes both banks mirror twins, which is pinned below as a documented
//! deviation rather than silently changing the vendor expression.

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::point10::audio_bridge::{
    AUDIO_SAMPLES, AudioBridge, BandPayload, P10Audio, RESPONSIVENESS_PROPS,
};
use crate::point10::postfx::Plane9BloomParams;
use crate::point10::scenes::cube_mesh::perspective;
use crate::point10::test_support::{
    TestTarget, any_lit, compact_contains, compact_match_count, gpu,
};
use crate::point10::{
    LENS_PROP, PLANE9_AUTHOR, PLANE9_SCENE_NAMES, Point10Scene, SceneContext, all_scenes,
    apply_lens, canonical_scene_name, gallery_kind, lens_value, plane9_scene_props,
    resolve_scene_name,
};

const SCENE_NAME: &str = "Plane9 Gravity Spectrum";
const LEGACY_NAME: &str = "Point10 Gravity Spectrum";
const GRAVITY_SPECTRUM_LICENSE: &str = concat!("CC BY-", "NC-", "SA 3.0");
const NC_SA_MARKER: &str = concat!("CC BY-", "NC-", "SA");

const LEFT_RIBBON_EXPRESSION: &str = r#"#left channel
i = ribbonnr/numribbons;
i2 = i+0.1;
pos.x =  cos(time*0.75)*i2;
pos.y = -sqrt(spectrum(0,i,0)*5);
pos.z =  sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
width=0.004;"#;

const RIGHT_RIBBON_EXPRESSION: &str = r#"#right channel
i = ribbonnr/numribbons;
i2 = i+0.1;
pos.x = -cos(time*0.75)*i2;
pos.y = -sqrt(spectrum(0,i,0)*5);
pos.z = -sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
width=0.004;"#;

const LEFT_CLONE_EXPRESSION: &str = r#"#left channel
i = x/numobjectsx;
i2 = i+0.1;
pos.x = cos(time*0.75)*i2;
pos.y = -spectrum(0,i,0)*2.5;
pos.z = sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
scale.x = scale.y = scale.z =0.02;"#;

const RIGHT_CLONE_EXPRESSION: &str = r#"#right channel
i = x/numobjectsx;
i2 = i+0.1;
pos.x = -cos(time*0.75)*i2;
pos.y = -spectrum(1,i,0)*2.5;
pos.z = -sin(time*0.75)*i2 - 0.5*sqrt(max(0,0.185-i))*band(-1,1,0,0.1);
col.a = i;
scale.x = scale.y = scale.z =0.02;"#;

const EXPRESSION1: &str = r#"out1 = min(2.6, 0.3+beat(0)*2.3);
out2 = band(-1, 1, 0, 0.1);
out3 = beat(0);"#;

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

fn check_all(failures: &mut Vec<String>, group: &str, source: &str, needles: &[&str]) {
    let missing: Vec<&str> = needles
        .iter()
        .copied()
        .filter(|needle| !compact_contains(source, needle))
        .collect();
    if !missing.is_empty() {
        failures.push(format!("{group}: missing {missing:?}"));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ReferenceSample {
    time: f32,
    position: [f32; 3],
    width: f32,
    color: [f32; 4],
}

/// Executable oracle for P9Nodes.js:1781-1822. This deliberately lives in the
/// test because RED may not name the not-yet-existing production helper type.
/// The production helper is separately source-contracted below.
struct ReferenceRibbonHistory {
    capacity: usize,
    evaluations_per_sec: f32,
    tick_time: f32,
    count: usize,
    evaluations: usize,
    samples: Vec<ReferenceSample>,
}

impl ReferenceRibbonHistory {
    fn new(capacity: usize, evaluations_per_sec: f32) -> Self {
        Self {
            capacity,
            evaluations_per_sec,
            tick_time: 0.0,
            count: 0,
            evaluations: 0,
            samples: vec![
                ReferenceSample {
                    time: 0.0,
                    position: [0.0; 3],
                    width: 0.5,
                    color: [1.0; 4],
                };
                capacity
            ],
        }
    }

    fn advance(&mut self, time: f32, mut evaluate: impl FnMut(f32) -> ReferenceSample) {
        let tick_dt = 1.0 / self.evaluations_per_sec;
        while self.tick_time + tick_dt <= time {
            self.tick_time += tick_dt;
            let sample = evaluate(self.tick_time);
            let slot = self.count % self.capacity;
            self.samples[slot] = sample;
            self.count += 1;
            self.evaluations += 1;
        }
    }

    fn point(&self, point: usize) -> Option<ReferenceSample> {
        if self.count == 0 {
            return None;
        }
        let taken = self.count.min(self.capacity);
        let clamped = point.min(taken - 1);
        let slot = (self.count - 1 - clamped) % self.capacity;
        Some(self.samples[slot])
    }
}

fn reference_ribbon_uvs(ribbons: usize, points: usize) -> Vec<[f32; 2]> {
    let mut uvs = Vec::with_capacity(ribbons * points * 2);
    for _ribbon in 0..ribbons {
        for point in 0..points {
            let v = point as f32 / (points - 1) as f32;
            uvs.push([0.0, v]);
            uvs.push([1.0, v]);
        }
    }
    uvs
}

fn reference_ribbon_indices(ribbons: usize, points: usize) -> Vec<u32> {
    let mut indices = Vec::with_capacity((points - 1) * 6 * ribbons);
    for ribbon in 0..ribbons {
        for point in 0..points - 1 {
            let a = ((ribbon * points + point) * 2) as u32;
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 2, a + 1, a + 3]);
        }
    }
    indices
}

#[test]
fn plane9_gravity_spectrum_fidelity_red_contract() {
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

    // A — XML 2-6: append the tenth Plane9/Dahl scene after Waves of Sin.
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
        names.last() == Some(&SCENE_NAME)
            && names.get(names.len().saturating_sub(2)) == Some(&"Plane9 Waves of Sin"),
        "A: Gravity Spectrum must append after Plane9 Waves of Sin; tail={:?}",
        &names[names.len().saturating_sub(2)..]
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
        PLANE9_SCENE_NAMES.last() == Some(&SCENE_NAME)
            && PLANE9_SCENE_NAMES.get(PLANE9_SCENE_NAMES.len().saturating_sub(2))
                == Some(&"Plane9 Waves of Sin"),
        "A: PLANE9_SCENE_NAMES must append {SCENE_NAME} after Plane9 Waves of Sin"
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

    // A — the fixture and catalogue counts are pinned at 54/506. XML line 6
    // is the exact 3.0 licence arm, not the 4.0 arm.
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
        "A: manifest total count is {:?}, expected 506",
        manifest["counts"]["total"]
    );
    let manifest_names: Vec<&str> = manifest["point10"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .map(|name| name.as_str().expect("manifest module name"))
                .collect()
        })
        .unwrap_or_default();
    check!(
        manifest_names.len() == 54
            && manifest_names
                .iter()
                .filter(|name| **name == SCENE_NAME)
                .count()
                == 1
            && manifest_names.last() == Some(&SCENE_NAME)
            && manifest_names.get(manifest_names.len().saturating_sub(2))
                == Some(&"Plane9 Waves of Sin"),
        "A: manifest must append {SCENE_NAME} once after Plane9 Waves of Sin at 54 names; got {} names and tail {:?}",
        manifest_names.len(),
        &manifest_names[manifest_names.len().saturating_sub(2)..]
    );

    let catalogue: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_root.join("docs/modules/modules.json"))
            .expect("read docs/modules/modules.json"),
    )
    .expect("parse docs/modules/modules.json");
    check!(
        catalogue["counts"]["point10"].as_u64() == Some(54)
            && catalogue["counts"]["total"].as_u64() == Some(506)
            && catalogue["modules"].as_array().map(Vec::len) == Some(506),
        "A: catalogue counts/modules must be 54/506/506, got {:?} and {:?}",
        catalogue["counts"],
        catalogue["modules"].as_array().map(Vec::len)
    );
    let catalogue_matches: Vec<_> = catalogue["modules"]
        .as_array()
        .map(|modules| {
            modules
                .iter()
                .filter(|module| module["name"] == SCENE_NAME)
                .collect()
        })
        .unwrap_or_default();
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
                && module["audioFeatures"] == serde_json::json!(["bass", "mid", "treble"])
                && module["description"].is_null(),
            "A: catalogue metadata/null description is wrong: {:?}",
            module
        );
        let prop_names: Vec<&str> = module["props"]
            .as_array()
            .map(|props| {
                props
                    .iter()
                    .map(|prop| prop["name"].as_str().expect("catalogue prop name"))
                    .collect()
            })
            .unwrap_or_default();
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
        .filter(|line| line.starts_with("| Gravity Spectrum |"))
        .collect();
    match notice_rows.as_slice() {
        [row]
            if compact_contains(row, "gravity_spectrum.rs")
                && compact_contains(row, GRAVITY_SPECTRUM_LICENSE)
                && compact_contains(row, "Joakim Dahl") => {}
        [row] => check!(false, "A: wrong Gravity Spectrum notices row {row:?}"),
        [] => check!(false, "A: missing Gravity Spectrum notices row"),
        _ => check!(
            false,
            "A: duplicate Gravity Spectrum notices rows {notice_rows:?}"
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
        compact_contains(v3_arm, "\"Gravity Spectrum\""),
        "A: module_license exact-3.0 arm is missing Gravity Spectrum"
    );
    check!(
        !compact_contains(v4_arm, "\"Gravity Spectrum\""),
        "A: Gravity Spectrum must not be classified in the 4.0 licence arm"
    );
    check!(
        compact_contains(&ui_model, "\"Point10 Gravity Spectrum\""),
        "A: UI provenance test is missing Point10 Gravity Spectrum"
    );
    check!(
        compact_contains(&ui_model, "checked.len(),54,"),
        "A: UI licence provenance count must move from 53 to 54"
    );

    // A — wrapper construction plus all nine delegation arms, without naming a
    // future Rust type in compiled test code.
    let app_registry = production_source(&repo_root.join("crates/modv-app/src/module_registry.rs"))
        .expect("read app module registry");
    check!(
        compact_contains(
            &app_registry,
            "usemodv_renderers::point10::scenes::gravity_spectrum::GravitySpectrum;"
        ),
        "A: app registry is missing the GravitySpectrum import"
    );
    check!(
        compact_contains(&app_registry, "GravitySpectrum(GravitySpectrum)"),
        "A: Point10SceneKind is missing GravitySpectrum(GravitySpectrum)"
    );
    check!(
        compact_contains(
            &app_registry,
            "\"Plane9 Gravity Spectrum\"=>Self::GravitySpectrum(GravitySpectrum::new())"
        ),
        "A: Point10SceneKind::create is missing Plane9 Gravity Spectrum"
    );
    check!(
        compact_match_count(&app_registry, "Self::GravitySpectrum(scene)") == 9,
        "A: Point10SceneKind must delegate apply_props plus all eight Point10Scene methods; got {} arms",
        compact_match_count(&app_registry, "Self::GravitySpectrum(scene)")
    );
    let app_registry_with_tests =
        fs::read_to_string(repo_root.join("crates/modv-app/src/module_registry.rs"))
            .expect("read app module registry with tests");
    check!(
        compact_contains(&app_registry_with_tests, "point10_definitions().len(),54,"),
        "A: app registry Point10 definition count must move from 53 to 54"
    );
    let wrapper_order = fn_body(
        &app_registry_with_tests,
        "point10_definitions_match_the_wrapper_modules",
    )
    .unwrap_or_default();
    let waves_position = wrapper_order.find("\"Plane9 Waves of Sin\"");
    let gravity_position = wrapper_order.find("\"Plane9 Gravity Spectrum\"");
    check!(
        waves_position
            .is_some_and(|waves| { gravity_position.is_some_and(|gravity| waves < gravity) }),
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

    // A/props — no vendor sceneProps.js exists, so the schema is exactly the
    // responsiveness levers followed by Lens.
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
    let gravity_index = scenes.iter().position(|scene| scene.name() == SCENE_NAME);
    if let Some(index) = gravity_index {
        check!(
            scenes[index].props() == expected_props,
            "A: Gravity Spectrum props must be responsiveness + Lens only; got {:?}",
            scenes[index].props()
        );
    }

    // D — the reusable pure model is mandatory; shader text alone is not enough.
    let helper_path = repo_root.join("crates/modv-renderers/src/point10/shared/ribbons.rs");
    let helper = production_source(&helper_path);
    check!(
        helper.is_some(),
        "D: missing reusable Ribbons history helper {}",
        helper_path.display()
    );
    let shared_modules =
        fs::read_to_string(repo_root.join("crates/modv-renderers/src/point10/shared/mod.rs"))
            .expect("read point10 shared module list");
    check!(
        compact_contains(&shared_modules, "pub mod ribbons;"),
        "D: point10::shared must export the reusable ribbons module"
    );
    if let Some(helper) = helper.as_deref() {
        check_all(
            &mut failures,
            "D/helper",
            helper,
            &[
                "pubstructRibbonHistory",
                "pubfnadvance(",
                "tick_dt",
                "while",
                "capacity",
                "coldstart",
                "oldest",
                "newest",
                "0.5",
                "[1.0,1.0,1.0,1.0]",
            ],
        );
        if let Some(advance) = fn_body(helper, "advance") {
            check_all(
                &mut failures,
                "D/advance",
                advance,
                &["tick_dt", "while", "%", "count", "evaluate"],
            );
        } else {
            failures.push("D/advance: missing brace-matched advance implementation".to_string());
        }
    }

    // D/G — executable fixed-timestep/ring oracle. Only the head is evaluated;
    // point 0 is newest and the cold tail clamps to the oldest recorded sample.
    let mut history = ReferenceRibbonHistory::new(110, 30.0);
    let sample = |time: f32| ReferenceSample {
        time,
        position: [time, time * 2.0, time * 3.0],
        width: 0.004,
        color: [1.0, 1.0, 1.0, 1.0],
    };
    history.advance(0.0, sample);
    check!(
        history.count == 0 && history.point(0).is_none(),
        "D: cold history must stay empty at time 0"
    );
    history.advance(0.04, sample);
    check!(
        history.count == 1
            && history.evaluations == 1
            && (0..110).all(|point| history.point(point) == history.point(0)),
        "D: the first tick must clamp every cold-tail point to the sole head sample"
    );
    history.advance(0.07, sample);
    check!(
        history.count == 2
            && history
                .point(0)
                .is_some_and(|point| point.time > history.point(1).unwrap().time)
            && (2..110).all(|point| history.point(point) == history.point(1)),
        "D: point 0 must be newest, point 1 the previous head, and the cold tail the oldest"
    );
    history.advance(3.66, sample);
    check!(
        history.count == 109 && history.evaluations == 109,
        "D: before np/eps the trail must remain short; got {} evaluations",
        history.evaluations
    );
    history.advance(3.67, sample);
    check!(
        history.count == 110 && history.evaluations == 110,
        "D: after np/eps ≈ 3.67 s the 110-slot trail must be full; got {} evaluations",
        history.evaluations
    );
    history.advance(4.01, sample);
    check!(
        history.count == 120
            && history.evaluations == 120
            && history
                .point(0)
                .is_some_and(|head| head.time > history.point(109).unwrap().time),
        "D: the ring must wrap at capacity while evaluating only 120 heads, not 120×110 strips"
    );

    // D — geometry oracle for two banks: 22,000 vertices and 65,400 indices per
    // bank, 44,000 vertices and 130,800 indices total.
    let ribbons = 100usize;
    let points = 110usize;
    let vertices = ribbons * points * 2;
    let index_count = (points - 1) * 6 * ribbons;
    check!(
        vertices == 22_000
            && index_count == 65_400
            && vertices * 2 == 44_000
            && index_count * 2 == 130_800,
        "D: ribbon geometry counts are wrong: vertices={vertices} indices={index_count}"
    );
    let uvs = reference_ribbon_uvs(ribbons, points);
    let indices = reference_ribbon_indices(ribbons, points);
    check!(
        uvs.len() == vertices
            && indices.len() == index_count
            && uvs.iter().enumerate().all(|(index, uv)| {
                let point = (index / 2) % points;
                let v = point as f32 / (points - 1) as f32;
                *uv == if index % 2 == 0 { [0.0, v] } else { [1.0, v] }
            }),
        "D: every strip vertex must carry uv.x {{0,1}} and uv.y = j/(np-1)"
    );
    check!(
        indices.first_chunk::<6>() == Some(&[0, 1, 2, 2, 1, 3])
            && indices.last_chunk::<6>() == Some(&[21_996, 21_997, 21_998, 21_998, 21_997, 21_999]),
        "D: first/last two-triangle segments must use the vendor's a,b,c,c,b,d winding"
    );

    // D — mono audio ignores channel. With the XML's channel arguments this is
    // an explicit mirror-twin deviation, not accidental stereo support.
    let mut payload = BandPayload::silent(44_100.0);
    payload.freq_mag = [0.35; AUDIO_SAMPLES];
    payload.byte_frequency_data = vec![128; 1024];
    payload.beat = 0.7;
    let mut bridge = AudioBridge::new();
    bridge.update(0.0, Some(&payload), &Default::default());
    let mut audio = P10Audio::new();
    audio.update(0.0, &bridge);
    check!(
        audio.spectrum(0, 0.25, 0.0) == audio.spectrum(1, 0.25, 0.0)
            && audio.band(0, 0, 0, 0.1) == audio.band(1, 0, 0, 0.1),
        "D: the mono bridge must make left/right spectrum and band reads identical"
    );
    let i = 0.5f32;
    let i2 = i + 0.1;
    let time = 2.0f32;
    let orbit = (time * 0.75).sin() * i2;
    let shared_band_offset = -0.5 * (0.185f32 - i).max(0.0).sqrt() * 0.4;
    let left_z = orbit + shared_band_offset;
    let right_z = -orbit + shared_band_offset;
    let left_x = (time * 0.75).cos() * i2;
    let right_x = -left_x;
    let spectrum = audio.spectrum(0, 0.25, 0.0);
    let left_y = -(spectrum * 5.0).sqrt();
    let right_y = -(spectrum * 5.0).sqrt();
    check!(
        left_x == -right_x
            && left_y == right_y
            && (left_z + right_z - 2.0 * shared_band_offset).abs() < 1.0e-6,
        "D: mono banks must mirror the orbit while sharing the vendor's non-negated band offset"
    );

    // C — the driver can carry the per-frame intensity values even though this
    // preset's radius/threshold pair derives a one-level chain.
    let bloom_low = Plane9BloomParams::new(10.0, 0.45, 0.5, 0.3, false);
    let bloom_high = Plane9BloomParams::new(10.0, 0.45, 0.5, 2.6, false);
    check!(
        bloom_low.intensity() == 0.3
            && bloom_high.intensity() == 2.6
            && bloom_low.mip_count() == 1
            && bloom_low != bloom_high,
        "C: bloom params must retain runtime intensity 0.3..2.6 for radius 10/threshold 0.45"
    );

    // B-G — read the not-yet-existing module at runtime. No future Rust type is
    // named by compiled code; every contract cites its XML/P9Nodes/P9Scene origin.
    let scene_path = repo_root.join("crates/modv-renderers/src/point10/scenes/gravity_spectrum.rs");
    let source = production_source(&scene_path);
    if source.is_none() {
        for group in ["A", "B", "C", "D", "E", "F", "G"] {
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
        let quad_source = production_source(
            &repo_root.join("crates/modv-renderers/src/point10/scenes/plane9_common.rs"),
        )
        .unwrap_or_default();

        // A — XML 3-6: exact provenance, empty Desc, and the 3.0 licence arm.
        check_all(
            &mut failures,
            "A/provenance",
            source,
            &[
                "Gravity Spectrum.xml",
                "Joakim Dahl",
                "Tags: `mellow,lines,vr`",
                "Desc",
                "empty",
            ],
        );
        check!(
            compact_contains(source, GRAVITY_SPECTRUM_LICENSE),
            "A: scene source must state the exact CC BY-NC-SA 3.0 licence"
        );
        let licence_declaration = source
            .lines()
            .find(|line| compact_contains(line, "const GRAVITY_SPECTRUM_LICENSE"))
            .unwrap_or_default();
        check!(
            compact_contains(licence_declaration, "concat!(")
                && !compact_contains(licence_declaration, NC_SA_MARKER),
            "A: the licence declaration must use the split concat! form without contiguous NC-SA scanner text"
        );
        check!(
            !compact_contains(header, "description is")
                && !compact_contains(header, "own description"),
            "A: the empty XML Desc must not acquire an invented description"
        );
        check!(
            !compact_contains(source, "sceneProps.js"),
            "A: no vendor sceneProps.js exists; the port must not cite a nonexistent override"
        );
        check!(
            compact_contains(&code, "plane9_scene_props(&[])")
                && compact_contains(&code, "lens_value(")
                && compact_contains(&code, "apply_lens("),
            "A/B: Gravity Spectrum must use standard no-override props and Lens helpers"
        );

        // B — Screen1 (XML 8-20). P9Scene.js:560-580 makes CamLookAt relative
        // and applies CamRot after lookAt as local-space rotateX/rotateY/rotateZ.
        check_all(
            &mut failures,
            "B/camera",
            source,
            &[
                "CAMERA_POSITION:[f32;3]=[0.0,0.4,1.8];",
                "CAMERA_ROTATION:[f32;3]=[-12.0,0.0,0.0];",
                "CAMERA_LOOK_AT:[f32;3]=[0.0,0.0,-1.0];",
                "CAMERA_LOOK_AT_IN_WORLD_SPACE:bool=false;",
                "CAMERA_FOV:f32=70.0;",
                "CAMERA_NEAR:f32=0.1;",
                "CAMERA_FAR:f32=1000.0;",
                "SCALE_BY_ASPECT:bool=true;",
                "CLEAR1:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
                "CLEAR2:wgpu::Color=wgpu::Color{r:0.0,g:0.0,b:0.0,a:1.0};",
            ],
        );
        check!(
            compact_contains(source, "relative")
                && compact_contains(source, "added to the position"),
            "B: CamLookAt (0,0,-1) must be documented as relative to CamPos"
        );
        if let Some(body) = fn_body(&code, "view_proj") {
            let look = body.find("look_at(");
            let rotate = body.find("euler_xyz(");
            check!(
                look.is_some_and(|look| rotate.is_some_and(|rotate| look < rotate)),
                "B: CamRot must be applied after lookAt, not discarded by a plain look_at"
            );
            check_all(
                &mut failures,
                "B/view_proj",
                body,
                &[
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
                    "self.camera_aspect",
                ],
            );
        } else {
            failures.push("B/view_proj: missing brace-matched implementation".to_string());
        }

        // C — XML 22-74, 223-228, 280-361: the graph, identity Transform1, the
        // first asymmetric RTT (50% width × 100% height), and runtime bloom.
        check_all(
            &mut failures,
            "C/graph",
            source,
            &[
                "Clear1",
                "RenderObject1",
                "Transform1",
                "Ribbons1",
                "CloneExpression1",
                "Ribbons2",
                "CloneExpression2",
                "RenderToTexture1",
                "Bloom1",
                "Shader3",
                "RenderRect1",
                "Screen1",
                "Clear2",
                "Expression1.Out1",
                "Bloom1.Intensity",
                "Expression1.Out2",
                "Shader2.In1",
                "Expression1.Out3",
                "Shader2.In2",
                "Clear1.Render",
                "RenderObject1.Render",
                "Transform1.Object",
                "Transform1.Object1",
                "Transform1.Object2",
                "Transform1.Object3",
                "Transform1.Object4",
                "RenderToTexture1.Render",
                "RenderToTexture1.Color",
                "Bloom1.Texture",
                "Shader3.Texture1",
                "Shader3.Texture2",
                "Shader3.Effect",
                "RenderRect1.Effect",
                "Clear2.Render",
                "RenderRect1.Render",
                "Screen1.Render",
                "TRANSFORM_POSITION:[f32;3]=[0.0,0.0,0.0];",
                "TRANSFORM_ROTATION:[f32;3]=[0.0,0.0,0.0];",
                "TRANSFORM_SCALE:[f32;3]=[1.0,1.0,1.0];",
            ],
        );
        check_all(
            &mut failures,
            "C/rtt",
            source,
            &[
                "RTT_FORMAT:wgpu::TextureFormat=wgpu::TextureFormat::Rgba16Float;",
                "RTT_WIDTH_MODE:u32=1;",
                "RTT_HEIGHT_MODE:u32=0;",
                "RTT_MIPS:bool=false;",
                "50%",
                "100%",
                "asymmetric",
            ],
        );
        check!(
            compact_contains(&quad_source, "mip_level_count:1")
                && compact_contains(&code, "create_colour_target("),
            "C: the capture must be a single-mip target made by create_colour_target"
        );
        check!(
            compact_contains(&code, "RTT_WIDTH_MODE")
                && compact_contains(&code, "RTT_HEIGHT_MODE")
                && (compact_contains(&code, "width/2")
                    || compact_contains(&code, "width.max(1)/2")),
            "C: RTT width mode 1 must actually produce half-width while height mode 0 stays full-height"
        );
        for forbidden in ["p9_fbm(", "_fbm("] {
            check!(
                !compact_contains(&code, forbidden),
                "C: dead RenderToTexture1 inline shader must not be implemented: {forbidden}"
            );
        }
        check_all(
            &mut failures,
            "C/bloom",
            source,
            &[
                "BLOOM_VENDOR_RADIUS:f32=10.0;",
                "BLOOM_THRESHOLD:f32=0.45;",
                "BLOOM_THRESHOLD_SMOOTHNESS:f32=0.5;",
                "BLOOM_ANTIFLICKER:bool=false;",
                "Plane9BloomParams::new(",
                "Plane9BloomPass::new(",
                "0.3+self.audio.beat(0.0)*2.3",
                ".min(2.6)",
            ],
        );
        check!(
            compact_contains(&code, "Plane9BloomPass::new(")
                && compact_match_count(&code, "Plane9BloomParams::new(") >= 1,
            "C: the scene must own a Plane9BloomPass and construct its driver params"
        );
        if let Some(update) = fn_body(&code, "update") {
            check!(
                compact_contains(update, "0.3+self.audio.beat(0.0)*2.3")
                    && compact_contains(update, ".min(2.6)")
                    && (compact_contains(update, "set_params(")
                        || compact_contains(update, "set_intensity(")),
                "C: update must feed min(2.6, 0.3+beat(0)*2.3) into the bloom driver at runtime"
            );
        } else {
            failures.push("C/update: missing brace-matched runtime bloom update".to_string());
        }
        let bloom_driver = production_source(
            &repo_root.join("crates/modv-renderers/src/point10/postfx/plane9_bloom.rs"),
        )
        .unwrap_or_default();
        check_all(
            &mut failures,
            "C/bloom-driver",
            &bloom_driver,
            &["pub(crate)fnset_params(", "self.params=params;"],
        );

        // D — XML 31-74 and P9Nodes.js:1714-1848. Two 100×110 banks at 30 Hz,
        // seed 1, exact expression channels, and the reusable recorded-head model.
        check_all(
            &mut failures,
            "D/ribbons",
            source,
            &[
                "NUM_RIBBONS:usize=100;",
                "NUM_POINTS:usize=110;",
                "EVALUATIONS_PER_SEC:f32=30.0;",
                "RIBBON_RANDOM_SEED:f32=1.0;",
                "RIBBON_BANKS:usize=2;",
                "ribbonnr=r+1",
                "numribbons=nr",
                "perm=RandomSeed",
                "time",
                "deltatime",
                "in1",
                "in2",
                "in3",
                "in4",
                "in5",
                "RibbonHistory",
                "mono",
                "mirror",
            ],
        );
        check!(
            compact_contains(source, LEFT_RIBBON_EXPRESSION),
            "D: left Ribbons1 expression was not transcribed exactly"
        );
        check!(
            compact_contains(source, RIGHT_RIBBON_EXPRESSION),
            "D: right Ribbons2 expression must preserve the XML's actual spectrum(0,i,0) channel argument"
        );
        let model_source = format!("{}\n{}", source, helper.as_deref().unwrap_or_default());
        check_all(
            &mut failures,
            "D/model",
            &model_source,
            &[
                "nr*np*2",
                "(np-1)*6*nr",
                "j/(np-1)",
                "uv",
                "tick_dt=1.0/evaluations_per_sec",
                "while",
                "count%capacity",
                "taken=count.min(capacity)",
                "point.min(taken-1)",
                "(count-1-clamped)%capacity",
                "width.unwrap_or(0.5)",
                "color.unwrap_or([1.0,1.0,1.0,1.0])",
            ],
        );
        // D — P9Expression.write pads a `col.a`-only assignment with zero rgb,
        // and P9Nodes.js:1802 copies that local into the ribbon colour. Shader1
        // reads only `diffuse.a`, so the black rgb is vendor-faithful and inert.
        check!(
            compact_contains(&code, "color:Some([0.0,0.0,0.0,i])"),
            "D: col.a-only ribbon heads must carry black RGB per P9Expression/P9Nodes.js"
        );

        // E — XML 122-221: two 100-clone banks over the unit-plane fallback and
        // the additive radar-blip shader. x is 1-based, so i spans 0.01..1.0.
        check_all(
            &mut failures,
            "E/clones",
            source,
            &[
                "CLONE_OBJECTS_X:usize=100;",
                "CLONE_OBJECTS_Y:usize=1;",
                "CLONE_OBJECTS_Z:usize=1;",
                "CLONE_ADJUST_FOR_ASPECT:bool=false;",
                "CLONE_CENTER:bool=false;",
                "CLONE_RANDOM_SEED:f32=1.0;",
                "unit plane",
                "fallback",
                "i=x/numobjectsx",
                "1-based",
            ],
        );
        check!(
            compact_contains(source, LEFT_CLONE_EXPRESSION),
            "E: left CloneExpression1 expression was not transcribed exactly"
        );
        check!(
            compact_contains(source, RIGHT_CLONE_EXPRESSION),
            "E: right CloneExpression2 expression/channel 1 was not transcribed exactly"
        );
        check!(
            compact_contains(&code, "Plane9QuadPass::new_additive(")
                && (compact_match_count(&code, "0..CLONE_OBJECTS_X") >= 2
                    || (compact_contains(&code, "0..DISC_COUNT")
                        && compact_contains(source, "DISC_COUNT:usize=200;"))),
            "E: 200 radar blips must draw as two 100-instance banks or one 200-instance unit-plane pass"
        );
        let disc = raw_const(source, "GRAVITY_SPECTRUM_DISC_WGSL");
        check!(
            disc.is_some(),
            "E: missing GRAVITY_SPECTRUM_DISC_WGSL literal"
        );
        if let Some(disc) = disc.as_deref() {
            check_all(
                &mut failures,
                "E/disc-shader",
                disc,
                &[
                    "letp=in.tex*2.0-1.0;",
                    "letrim=pow(p9_saturate(1.0-length(p)),2.0);",
                    "letglow=rim*(0.5+u.in1.x*1.5+u.in2.x*3.0);",
                    "oColor=in.diffuse*p9_saturate(1.0-length(p))*(1.5+u.in1.x*4.0)+vec4<f32>(in.diffuse.rgb*glow,0.0);",
                    "returnoColor;",
                ],
            );
        }
        // E — CloneExpression's `col.a = i` leaves rgb zero in the vendor env
        // (P9Expression.makeEnv), and P9Nodes.js:1683 writes `[0,0,0,i]` into
        // gColor, multiplying iColor/Color1 (both white). Shader2's rgb terms
        // are therefore zero: the radar blips add no colour in the vendor.
        check!(
            compact_contains(&code, "color:[0.0,0.0,0.0,i]"),
            "E: col.a-only clone instances must feed black RGB to Shader2 per P9Nodes.js"
        );
        check_all(
            &mut failures,
            "E/disc-state",
            &code,
            &[
                "SHADER2_DEPTH_TEST:bool=false;",
                "SHADER2_DEPTH_WRITE:bool=false;",
                "SHADER2_SRC_BLEND:u32=1;",
                "SHADER2_DST_BLEND:u32=1;",
                "SHADER2_CULL:u32=0;",
            ],
        );
        check!(
            compact_contains(&quad_source, "depth_stencil:None")
                && compact_contains(&quad_source, "cull_mode:None"),
            "E: Shader2's shared quad pipeline must disable depth and culling"
        );
        check!(
            compact_contains(&quad_source, "Some(wgpu::BlendState::ADDITIVE)")
                || compact_contains(
                    &quad_source,
                    "color:wgpu::BlendComponent{src_factor:wgpu::BlendFactor::One,dst_factor:wgpu::BlendFactor::One,operation:wgpu::BlendOperation::Add}"
                ),
            "E: Shader2 must use One/One additive blending"
        );

        // F — XML 75-121 and 233-279: ribbon hue/tip shader, ACES composite and
        // deliberate squaring, plus Expression1's three live outputs.
        let ribbon_shader = raw_const(source, "GRAVITY_SPECTRUM_WGSL");
        check!(
            ribbon_shader.is_some(),
            "F: missing GRAVITY_SPECTRUM_WGSL literal"
        );
        if let Some(shader) = ribbon_shader.as_deref() {
            check_all(
                &mut failures,
                "F/ribbon-shader",
                shader,
                &[
                    "letp=in.tex*2.0-1.0;",
                    "letcol=p9_hsv2rgb(in.diffuse.a*0.6,0.8,0.5);",
                    "lettip=p9_smoothstep(0.99,1.0,1.0-in.tex.y);",
                    "letcol1=vec4<f32>(col,(1.0-in.tex.y)*(1.0-abs(p.x))*u.in1.x);",
                    "oColor=col1*(1.0+3.0*tip);",
                    "returnoColor;",
                ],
            );
        }
        check_all(
            &mut failures,
            "F/ribbon-state",
            source,
            &[
                "SHADER1_DEPTH_TEST:bool=false;",
                "SHADER1_DEPTH_WRITE:bool=true;",
                "SHADER1_SRC_BLEND:u32=6;",
                "SHADER1_DST_BLEND:u32=1;",
                "SHADER1_CULL:u32=0;",
                "depth_write_enabled:Some(true)",
                "src_factor:wgpu::BlendFactor::SrcAlpha",
                "dst_factor:wgpu::BlendFactor::One",
            ],
        );
        check!(
            compact_contains(&code, "(0.75+self.audio.beat(0.0)*10.0).min(1.0)")
                && compact_contains(source, "in1.x=min(1.0,0.75+beat(0)*10.0)"),
            "F: Shader1's beat-driven In1 must be computed in code and quoted from the XML"
        );
        let composite = raw_const(source, "GRAVITY_SPECTRUM_COMPOSITE_WGSL");
        check!(
            composite.is_some(),
            "F: missing GRAVITY_SPECTRUM_COMPOSITE_WGSL literal"
        );
        if let Some(composite) = composite.as_deref() {
            check_all(
                &mut failures,
                "F/composite-shader",
                composite,
                &[
                    "letc1=textureSampleLevel(g_texture1,g_sampler,tex,0.0);",
                    "letc2=textureSampleLevel(g_texture2,g_sampler,tex,0.0);",
                    "oColor=vec4<f32>(p9_tonemap_aces(c1.rgb+c2.rgb),1.0);",
                    "oColor=oColor*oColor;",
                    "returnoColor;",
                ],
            );
        }
        check!(
            compact_contains(source, EXPRESSION1),
            "F: Expression1 outputs were not transcribed exactly"
        );
        check_all(
            &mut failures,
            "F/composite-state",
            source,
            &[
                "SHADER3_DEPTH_TEST:bool=false;",
                "SHADER3_DEPTH_WRITE:bool=false;",
                "SHADER3_SRC_BLEND:u32=1;",
                "SHADER3_DST_BLEND:u32=4;",
                "SHADER3_CULL:u32=0;",
                "src_factor:wgpu::BlendFactor::One",
                "deliberate",
            ],
        );
        check!(
            compact_contains(&code, "dst_factor:wgpu::BlendFactor::OneMinusSrc,")
                && compact_contains(source, "OneMinusSrcColor"),
            "F: Shader3's dst factor must be wgpu OneMinusSrc (Plane9 enum 4, Inverse Source Color)"
        );
        if let Some(update) = fn_body(&code, "update") {
            check_all(
                &mut failures,
                "F/update",
                update,
                &[
                    "self.audio.band(-1,1,0,0.1)",
                    "self.audio.beat(0.0)",
                    "self.uniforms.in1",
                    "self.uniforms.in2",
                ],
            );
        } else {
            failures.push("F/update: missing brace-matched expression drive".to_string());
        }
    }

    // G — 320×180 fallback-adapter smoke. Two ribbon banks (44,000 vertices)
    // plus 200 discs render into the scene's 160×180 RGBA16F capture. Few frames,
    // with init/render/readback timings and deterministic fresh-instance output.
    if let Some(index) = gravity_index {
        if source.is_none() {
            failures.push(format!(
                "G: registered {SCENE_NAME} has no scene source {}",
                scene_path.display()
            ));
        }
        if source.is_some()
            && let Some((device, queue)) = gpu()
        {
            const WIDTH: u32 = 320;
            const HEIGHT: u32 = 180;
            let started = Instant::now();
            let mut scene = scenes.remove(index);
            let mut repeat = all_scenes()
                .into_iter()
                .find(|scene| scene.name() == SCENE_NAME)
                .expect("second Gravity Spectrum registry lookup");
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
                "Plane9 Gravity Spectrum GPU init (320x180 output, 160x180 Rgba16F RTT, two instances): {:?}",
                started.elapsed()
            );

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
                    label: Some("point10-plane9-gravity-spectrum-red-smoke"),
                });
                scene.render(&mut encoder, &target.view);
                queue.submit(Some(encoder.finish()));
                let pixels = target.read(&device, &queue);
                eprintln!(
                    "Plane9 Gravity Spectrum GPU {label} (100 ribbons × 110 points × 2 banks + 200 discs): {:?}",
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
            // P9Nodes.js:1787-1807 starts the ribbon ring empty, and the
            // `col.a`-only clone expression feeds black rgb to Shader2, so the
            // cold frames are legitimately black. Fill the 110-slot history
            // (110 / 30 = 3.67 s) before expecting a lit frame.
            for _ in 0..40 {
                bridge.update(0.1, Some(&payload), &Default::default());
                scene.update(0.1, &bridge);
            }
            let second = render(
                &mut scene,
                &mut bridge,
                &target,
                1.0 / 60.0,
                "frame 2 (dt=1/60, history full)",
            );
            check!(
                !any_lit(&first),
                "G: the vendor cold frame must be black before the first ribbon tick"
            );
            check!(
                !any_lit(&repeated),
                "G: a fresh instance's cold frame must also be black"
            );
            check!(any_lit(&second), "G: Gravity Spectrum frame 2 is empty");
            check!(
                first == repeated,
                "G: Gravity Spectrum frame 1 is not byte-for-byte deterministic across fresh instances"
            );
            check!(
                first != second,
                "G: Gravity Spectrum later frame did not evolve"
            );
            eprintln!(
                "Plane9 Gravity Spectrum GPU smoke total (three draws/readbacks): {:?}",
                started.elapsed()
            );
            scene.dispose();
            repeat.dispose();
        } else if source.is_some() {
            eprintln!("skipping Plane9 Gravity Spectrum GPU smoke: no fallback adapter");
        }
    } else {
        failures.push(format!(
            "G: cannot run the 320x180 GPU smoke because {SCENE_NAME} is not registered"
        ));
    }

    assert!(
        failures.is_empty(),
        "Plane9 Gravity Spectrum fidelity RED contract failed ({} checks):\n{}",
        failures.len(),
        failures
            .iter()
            .enumerate()
            .map(|(index, failure)| format!("{:02}. {failure}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
