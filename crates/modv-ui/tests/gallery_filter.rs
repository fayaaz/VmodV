//! Pure Gallery filtering tests: the default view must show every module and
//! the category/search filters must stay exact without any tile cap.

use std::collections::BTreeSet;

use modv_ui::model::{EngineSnapshot, ModuleMetaView, gallery_preview_key};
use modv_ui::state::{GalleryCategory, UiState, filter_gallery_modules};

fn module(name: &str, module_type: &str, category: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        kind: module_type.to_string(),
        category: category.to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
        tags: Vec::new(),
    }
}

fn names<'a>(modules: &[&'a ModuleMetaView]) -> Vec<&'a str> {
    modules.iter().map(|module| module.name.as_str()).collect()
}

fn mixed_modules() -> Vec<ModuleMetaView> {
    vec![
        module("Bands", "shader", "effects"),
        module("Chromatic Line", "2d", ""),
        module("Tunnel", "isf", "3d-scenes"),
        module("Ring of Fire", "shader", "effects"),
        module("Fire Chars", "2d", "2d-scenes"),
        module("Deep Sea", "isf", "3d-scenes"),
    ]
}

#[test]
fn default_gallery_is_the_all_category_with_an_empty_search() {
    let state = UiState::new();
    assert_eq!(state.gallery_category, GalleryCategory::All);
    assert!(state.search.is_empty());
    assert_eq!(GalleryCategory::default(), GalleryCategory::All);
}

#[test]
fn category_buttons_offer_all_first_then_the_three_sections() {
    assert_eq!(
        GalleryCategory::ALL,
        [
            GalleryCategory::All,
            GalleryCategory::Effects,
            GalleryCategory::TwoDScenes,
            GalleryCategory::ThreeDScenes,
        ]
    );
    assert_eq!(GalleryCategory::All.label(), "All");
}

#[test]
fn all_includes_effects_and_both_scene_kinds() {
    let modules = mixed_modules();
    let filtered = filter_gallery_modules(&modules, GalleryCategory::All, "");
    assert_eq!(
        names(&filtered),
        vec![
            "Bands",
            "Chromatic Line",
            "Tunnel",
            "Ring of Fire",
            "Fire Chars",
            "Deep Sea",
        ]
    );
}

#[test]
fn effects_excludes_two_d_and_three_d_scenes() {
    let modules = mixed_modules();
    let filtered = filter_gallery_modules(&modules, GalleryCategory::Effects, "");
    assert_eq!(names(&filtered), vec!["Bands", "Ring of Fire"]);
}

#[test]
fn scene_categories_only_include_their_own_section() {
    let modules = mixed_modules();
    let two_d = filter_gallery_modules(&modules, GalleryCategory::TwoDScenes, "");
    assert_eq!(names(&two_d), vec!["Chromatic Line", "Fire Chars"]);

    let three_d = filter_gallery_modules(&modules, GalleryCategory::ThreeDScenes, "");
    assert_eq!(names(&three_d), vec!["Tunnel", "Deep Sea"]);
}

#[test]
fn category_matching_follows_the_legacy_gallery_fallbacks() {
    assert!(GalleryCategory::All.matches("effects"));
    assert!(GalleryCategory::All.matches("2d-scenes"));
    assert!(GalleryCategory::All.matches("3d-scenes"));
    assert!(GalleryCategory::All.matches("something-unknown"));

    assert!(GalleryCategory::Effects.matches("effects"));
    assert!(!GalleryCategory::Effects.matches("2d-scenes"));
    assert!(!GalleryCategory::Effects.matches("3d-scenes"));

    assert!(GalleryCategory::TwoDScenes.matches("2d-scenes"));
    assert!(!GalleryCategory::TwoDScenes.matches("effects"));
    assert!(GalleryCategory::ThreeDScenes.matches("3d-scenes"));
    assert!(!GalleryCategory::ThreeDScenes.matches("2d-scenes"));

    // `key()` is empty for All and therefore must never be used for matching.
    assert_eq!(GalleryCategory::All.key(), "");
    assert_eq!(GalleryCategory::Effects.key(), "effects");
}

#[test]
fn unknown_explicit_categories_fall_back_by_module_type_but_still_match_all() {
    let modules = vec![
        module("Mystery 2D", "2d", "not-a-known-category"),
        module("Mystery ISF", "isf", "not-a-known-category"),
        module("Mystery 3D", "3d", "not-a-known-category"),
    ];
    assert_eq!(
        names(&filter_gallery_modules(&modules, GalleryCategory::All, "")),
        vec!["Mystery 2D", "Mystery ISF", "Mystery 3D"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::TwoDScenes,
            ""
        )),
        vec!["Mystery 2D"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::Effects,
            ""
        )),
        vec!["Mystery ISF"]
    );
}

#[test]
fn empty_search_returns_every_registered_module_without_a_tile_cap() {
    // The registry is 432 modules; the old Gallery silently stopped at 24.
    let modules: Vec<ModuleMetaView> = (0..432)
        .map(|index| {
            let (module_type, category) = match index % 3 {
                0 => ("shader", "effects"),
                1 => ("2d", "2d-scenes"),
                _ => ("isf", "3d-scenes"),
            };
            module(&format!("Module {index:03}"), module_type, category)
        })
        .collect();

    let filtered = filter_gallery_modules(&modules, GalleryCategory::All, "");
    assert_eq!(filtered.len(), 432);
    assert_eq!(filtered.first().unwrap().name, "Module 000");
    assert_eq!(filtered.last().unwrap().name, "Module 431");
}

#[test]
fn search_narrows_within_all_and_respects_the_category() {
    let modules = mixed_modules();
    let fire = filter_gallery_modules(&modules, GalleryCategory::All, "fire");
    assert_eq!(names(&fire), vec!["Ring of Fire", "Fire Chars"]);

    let fire_effects = filter_gallery_modules(&modules, GalleryCategory::Effects, "fire");
    assert_eq!(names(&fire_effects), vec!["Ring of Fire"]);

    let fire_scenes = filter_gallery_modules(&modules, GalleryCategory::ThreeDScenes, "fire");
    assert!(fire_scenes.is_empty());
}

#[test]
fn search_matches_author_case_insensitively_and_trims_whitespace() {
    let modules = vec![
        ModuleMetaView {
            author: "Alice".to_string(),
            ..module("Bands", "shader", "effects")
        },
        ModuleMetaView {
            author: "Bob".to_string(),
            ..module("Tunnel", "isf", "3d-scenes")
        },
    ];

    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "  ALICE "
        )),
        vec!["Bands"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "bob"
        )),
        vec!["Tunnel"]
    );
    assert!(
        filter_gallery_modules(&modules, GalleryCategory::All, "  ").len() == 2,
        "whitespace-only search must behave like an empty search"
    );
}

// --- Searching by module type (plain free text) -----------------------------
//
// Point10 scenes register with `module_type: "2d"` for JS/preset compatibility
// even though a Point10 renderer owns them; they are identified by their
// authoritative `kind`. A plain 2D module must therefore never match a Point10
// type search, despite sharing that `module_type`. There is no query language:
// the renderer kind is just another searchable term.

/// Like `module`, but for a Point10 scene: the renderer kind is authoritative
/// even though Point10 registers with the legacy `module_type` of `"2d"`.
fn module_by(name: &str, module_type: &str, category: &str, author: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        kind: "point10".to_string(),
        category: category.to_string(),
        author: author.to_string(),
        version: "1.0".to_string(),
        description: String::new(),
        tags: Vec::new(),
    }
}

fn typed_modules() -> Vec<ModuleMetaView> {
    vec![
        module("Bands", "shader", "effects"),
        module("Ring of Fire", "shader", "effects"),
        module("Chromatic Line", "2d", "2d-scenes"),
        module("Fire Chars", "2d", "2d-scenes"),
        module_by("Point10 Boxy", "2d", "2d-scenes", "Point10"),
        module_by("Point10 Rolling Fire", "2d", "2d-scenes", "Point10"),
        module("Tunnel", "isf", "3d-scenes"),
        module("Deep Sea", "isf", "3d-scenes"),
        module("Strobe Riot", "milkdrop", "effects"),
        module("Feedback Fractal", "milkdrop", "3d-scenes"),
    ]
}

#[test]
fn plain_isf_search_selects_only_isf_modules_case_insensitively() {
    let modules = typed_modules();
    for query in ["isf", "ISF", "  ISF  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Tunnel", "Deep Sea"],
            "type searches must be case-insensitive and tolerate surrounding whitespace: {query:?}"
        );
    }
}

#[test]
fn shader_and_milkdrop_searches_select_only_their_own_modules() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "shader"
        )),
        vec!["Bands", "Ring of Fire"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "milkdrop"
        )),
        vec!["Strobe Riot", "Feedback Fractal"]
    );
}

#[test]
fn point10_search_excludes_plain_2d_modules_sharing_the_module_type() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "point10"
        )),
        vec!["Point10 Boxy", "Point10 Rolling Fire"],
        "Point10 is a renderer kind: its modules register as module_type '2d', \
         so a filter that merely matches module_type would also return \
         'Chromatic Line' and 'Fire Chars'"
    );
}

#[test]
fn type_search_combines_with_free_text_and_the_gallery_category() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "isf sea"
        )),
        vec!["Deep Sea"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "sea isf"
        )),
        vec!["Deep Sea"],
        "the type term may appear before or after the free-text terms"
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::ThreeDScenes,
            "isf"
        )),
        vec!["Tunnel", "Deep Sea"]
    );
    assert!(
        filter_gallery_modules(&modules, GalleryCategory::Effects, "isf sea").is_empty(),
        "category filtering still applies on top of a type search"
    );
}

#[test]
fn unknown_plain_text_matches_nothing() {
    let modules = typed_modules();
    assert!(
        filter_gallery_modules(&modules, GalleryCategory::All, "bogus").is_empty(),
        "an unmatched search term must not fall back to an unfiltered list"
    );
}

#[test]
fn plain_text_search_keeps_its_original_name_and_author_semantics() {
    let modules = vec![
        module_by("Point10 Boxy", "2d", "2d-scenes", "Point10"),
        module("Fire Chars", "2d", "2d-scenes"),
    ];
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "  POINT10  "
        )),
        vec!["Point10 Boxy"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "modv"
        )),
        vec!["Fire Chars"]
    );
}

#[test]
fn plain_text_search_matches_module_types_and_default_tags() {
    let mut modules = typed_modules();
    // Keep the renderer kind detectable even when neither the name nor author
    // contains "Point10", and even though its legacy module_type is "2d".
    modules.push(module_by("Boxy", "2d", "2d-scenes", "Studio"));

    for query in ["isf", "  ISF  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Tunnel", "Deep Sea"],
            "plain module-type searches must be case-insensitive and trimmed: {query:?}"
        );
    }
    for query in ["shader", "  SHADER  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Bands", "Ring of Fire"],
            "plain module-type searches must select only shader modules: {query:?}"
        );
    }
    for query in ["point10", "  POINT10  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Point10 Boxy", "Point10 Rolling Fire", "Boxy"],
            "Point10 must match its renderer kind, not the legacy '2d' module_type: {query:?}"
        );
    }

    // None of these names or authors contain "effect" or "scene"; the matches
    // can only come from default tags derived from the Gallery categories.
    for query in ["effect", "  EFFECT  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Bands", "Ring of Fire", "Strobe Riot"],
            "effects must have a searchable default 'effect' tag: {query:?}"
        );
    }
    for query in ["scene", "  SCENE  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec![
                "Chromatic Line",
                "Fire Chars",
                "Point10 Boxy",
                "Point10 Rolling Fire",
                "Tunnel",
                "Deep Sea",
                "Feedback Fractal",
                "Boxy",
            ],
            "2D and 3D scene categories, including Point10, need a default \
             'scene' tag: {query:?}"
        );
    }
    for query in ["scene fire", "fire scene"] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Fire Chars", "Point10 Rolling Fire"],
            "free-text terms must combine across default tags and module names: {query:?}"
        );
    }
}

/// Like [`module`], but with an authoritative renderer kind independent of the
/// legacy `module_type` (all Point10/Plane9 scenes still register as `2d`).
fn module_with_kind(name: &str, kind: &str) -> ModuleMetaView {
    ModuleMetaView {
        kind: kind.to_string(),
        ..module(name, "2d", "2d-scenes")
    }
}

#[test]
fn gallery_renderer_kind_filters_separate_plane9_from_point10_without_alias_tiles() {
    const PLANE9: [&str; 34] = [
        "Plane9 Abstract Clouds",
        "Plane9 Blob Fade",
        "Plane9 Boxy",
        "Plane9 Cell Culture",
        "Plane9 Circle Crossover",
        "Plane9 Cloud Flight",
        "Plane9 Color Beams",
        "Plane9 Color Blob",
        "Plane9 Color Cube",
        "Plane9 Colors",
        "Plane9 Earth",
        "Plane9 Evil Box",
        "Plane9 Gradient",
        "Plane9 Hollow Cube",
        "Plane9 Hypno Toad",
        "Plane9 It Is Coming For You",
        "Plane9 Light Tunnel",
        "Plane9 Magic Smoke Box",
        "Plane9 Mirror Cube",
        "Plane9 Mirror Tunnel",
        "Plane9 Music Waves",
        "Plane9 Noise Warp",
        "Plane9 Rorschach",
        "Plane9 Sound Cube",
        "Plane9 Spiky",
        "Plane9 Square Tunnel",
        "Plane9 Stargate Event Horizon",
        "Plane9 Sun",
        "Plane9 The Cave",
        "Plane9 Though The Noise",
        "Plane9 Truchet Tentacles",
        "Plane9 Tv Noise",
        "Plane9 Wormhole",
        "Plane9 Xor",
    ];
    const POINT10: [&str; 18] = [
        "Point10 Around Plus",
        "Point10 Attractor Particles",
        "Point10 Ball Of Fire",
        "Point10 Chromatic Line",
        "Point10 Chromatic Wave",
        "Point10 DVD Room",
        "Point10 Hollow Cube Blur",
        "Point10 Hollow Cube Layers",
        "Point10 Hollow Cuboid",
        "Point10 Latent Backrooms",
        "Point10 Mycelium",
        "Point10 Particle Field",
        "Point10 Rolling Fire",
        "Point10 Smoke Room",
        "Point10 Spiral Particles",
        "Point10 Ultra HDR",
        "Point10 Water Ripple",
        "Point10 Waveform",
    ];

    // Accumulate every contract failure so one RED run demonstrates all of the
    // missing behaviour rather than stopping at the first unsupported token.
    let mut failures = Vec::new();

    // A — the renderer kind is matched as free text, exactly like any other
    // tag. Every current kind must remain selectable, with Plane9 joining
    // Point10 as a distinct kind. The tile names deliberately avoid the kind
    // word and the legacy `module_type` is neutral, so a match can only come
    // from the authoritative `kind` field.
    let kinds = ["shader", "2d", "point10", "plane9", "isf", "milkdrop"];
    let kind_modules: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| ModuleMetaView {
            module_type: "custom".to_string(),
            ..module_with_kind(&format!("Tile {}", (b'A' + index as u8) as char), kind)
        })
        .collect();
    for (index, kind) in kinds.iter().enumerate() {
        let filtered = filter_gallery_modules(&kind_modules, GalleryCategory::All, kind);
        let expected = format!("Tile {}", (b'A' + index as u8) as char);
        if names(&filtered) != vec![expected.as_str()] {
            failures.push(format!(
                "A: {kind:?} selected {:?}, expected only {expected:?}",
                names(&filtered)
            ));
        }
    }

    let mut modules = Vec::with_capacity(PLANE9.len() + POINT10.len());
    modules.extend(PLANE9.iter().map(|name| module_with_kind(name, "plane9")));
    modules.extend(POINT10.iter().map(|name| module_with_kind(name, "point10")));

    // B — Plane9 is exactly the thirty-four renamed/imported preset ports.
    let plane9 = filter_gallery_modules(&modules, GalleryCategory::All, "plane9");
    if names(&plane9) != PLANE9 {
        failures.push(format!(
            "B: plane9 selected {:?}, expected exactly {PLANE9:?}",
            names(&plane9)
        ));
    }

    // C — Point10 is exactly the other eighteen scenes, with no Plane9 leakage.
    let point10 = filter_gallery_modules(&modules, GalleryCategory::All, "point10");
    if names(&point10) != POINT10 {
        failures.push(format!(
            "C: point10 selected {:?}, expected exactly {POINT10:?}",
            names(&point10)
        ));
    }

    // D — the All view still contains all 52 canonical scene entries.
    let all = filter_gallery_modules(&modules, GalleryCategory::All, "");
    let expected_all: Vec<&str> = PLANE9.iter().chain(POINT10.iter()).copied().collect();
    if names(&all) != expected_all {
        failures.push(format!(
            "D: All selected {:?}, expected all 52 canonical scenes",
            names(&all)
        ));
    }

    // E — canonical entries have unique tile keys, and looking up a historical
    // alias resolves to the existing canonical entry instead of synthesising a
    // second tile. The registry-facing list deliberately contains no aliases.
    let snapshot = EngineSnapshot {
        registered: modules.clone(),
        ..Default::default()
    };
    let tile_keys: Vec<_> = all
        .iter()
        .map(|module| gallery_preview_key(&module.name))
        .collect();
    let unique_tile_keys: BTreeSet<_> = tile_keys.iter().cloned().collect();
    if tile_keys.len() != unique_tile_keys.len() {
        failures.push(format!(
            "E: All produced duplicate tile keys: {tile_keys:?}"
        ));
    }
    let canonical = snapshot.registered_module("Plane9 Light Tunnel");
    let legacy = snapshot.registered_module("Point10 Light Tunnel");
    if !matches!((canonical, legacy), (Some(canonical), Some(legacy)) if std::ptr::eq(canonical, legacy))
    {
        failures.push(format!(
            "E: legacy lookup resolved to {legacy:?}, expected the same entry as {canonical:?}"
        ));
    }
    if snapshot
        .registered
        .iter()
        .any(|module| module.name == "Point10 Light Tunnel")
    {
        failures.push("E: the canonical Gallery list contains a legacy alias tile".to_string());
    }

    assert!(
        failures.is_empty(),
        "Gallery Plane9/Point10 renderer-kind contract failed:\n- {}",
        failures.join("\n- ")
    );
}

/// The UI resolves aliases with a `Point10 ` -> `Plane9 ` prefix fallback so
/// `modv-ui` stays free of the GPU-heavy renderer crate. Pin that duplication
/// to the renderer's single source of truth: every canonical Plane9 port's
/// legacy name must resolve to the same entry, and unrelated Point10 scenes
/// must keep resolving to themselves.
#[test]
fn legacy_plane9_alias_lookup_matches_renderer_roster() {
    use modv_renderers::point10::{
        PLANE9_PREFIX, PLANE9_SCENE_NAMES, POINT10_PREFIX, canonical_scene_name, gallery_kind,
    };

    let canonical_modules: Vec<_> = PLANE9_SCENE_NAMES
        .iter()
        .map(|name| module_with_kind(name, &gallery_kind(name, "point10")))
        .collect();
    let point10_modules = [module_with_kind("Point10 Around Plus", "point10")];
    let mut all_modules = canonical_modules.clone();
    all_modules.extend(point10_modules.iter().cloned());
    let snapshot = EngineSnapshot {
        registered: all_modules,
        ..Default::default()
    };

    for canonical in PLANE9_SCENE_NAMES {
        let suffix = canonical
            .strip_prefix(PLANE9_PREFIX)
            .expect("roster entry has the Plane9 prefix");
        let legacy = format!("{POINT10_PREFIX}{suffix}");

        // The renderer says this legacy name is an alias of `canonical`.
        assert_eq!(canonical_scene_name(&legacy), Some(*canonical));
        // The UI parses the same prefix pair and lands on the identical entry.
        let from_legacy = snapshot
            .registered_module(&legacy)
            .unwrap_or_else(|| panic!("no alias entry for {legacy}"));
        let from_canonical = snapshot
            .registered_module(canonical)
            .unwrap_or_else(|| panic!("no canonical entry for {canonical}"));
        assert!(std::ptr::eq(from_legacy, from_canonical));
        assert_eq!(from_legacy.name, *canonical);
        assert_eq!(gallery_kind(canonical, "point10"), "plane9");
    }

    // A Point10-authored scene is neither aliased nor classified as Plane9.
    let around = snapshot
        .registered_module("Point10 Around Plus")
        .expect("unrelated Point10 scene resolves to itself");
    assert_eq!(around.name, "Point10 Around Plus");
    assert_eq!(around.kind, "point10");
    assert_eq!(canonical_scene_name("Point10 Around Plus"), None);
    assert_eq!(gallery_kind("Point10 Around Plus", "point10"), "point10");
}

#[test]
fn search_terms_are_case_insensitive() {
    let modules = vec![
        module_with_kind("A", "plane9"),
        module_with_kind("B", "point10"),
    ];
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "PLANE9"
        )),
        vec!["A"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "PoInT10"
        )),
        vec!["B"]
    );
    assert!(filter_gallery_modules(&modules, GalleryCategory::All, "nope").is_empty());
}
