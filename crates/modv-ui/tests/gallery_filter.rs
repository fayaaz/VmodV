//! Pure Gallery filtering tests: the default view must show every module and
//! the category/search filters must stay exact without any tile cap.

use modv_ui::model::ModuleMetaView;
use modv_ui::state::{GalleryCategory, UiState, filter_gallery_modules};

fn module(name: &str, module_type: &str, category: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        category: category.to_string(),
        author: "modV".to_string(),
        version: "1.0".to_string(),
        description: String::new(),
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

// --- Searching by module type (`type:` queries) -----------------------------
//
// Point10 scenes register with `module_type: "2d"` for JS/preset compatibility
// even though a Point10 renderer owns them; they are identified by their
// `Point10` author. A plain 2D module must therefore never match a Point10
// type query, despite sharing that `module_type`.

/// Like `module`, but lets a fixture vary its author.
fn module_by(name: &str, module_type: &str, category: &str, author: &str) -> ModuleMetaView {
    ModuleMetaView {
        name: name.to_string(),
        module_type: module_type.to_string(),
        category: category.to_string(),
        author: author.to_string(),
        version: "1.0".to_string(),
        description: String::new(),
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
fn type_isf_query_selects_only_isf_modules_case_insensitively() {
    let modules = typed_modules();
    for query in ["type:isf", "TYPE:ISF", "  type:ISF  "] {
        assert_eq!(
            names(&filter_gallery_modules(
                &modules,
                GalleryCategory::All,
                query
            )),
            vec!["Tunnel", "Deep Sea"],
            "type tokens must be case-insensitive and tolerate surrounding whitespace: {query:?}"
        );
    }
}

#[test]
fn type_shader_and_milkdrop_queries_select_only_their_own_modules() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "type:shader"
        )),
        vec!["Bands", "Ring of Fire"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "type:milkdrop"
        )),
        vec!["Strobe Riot", "Feedback Fractal"]
    );
}

#[test]
fn type_point10_query_excludes_plain_2d_modules_sharing_the_module_type() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "type:point10"
        )),
        vec!["Point10 Boxy", "Point10 Rolling Fire"],
        "Point10 is a renderer kind: its modules register as module_type '2d', \
         so a filter that merely matches module_type would also return \
         'Chromatic Line' and 'Fire Chars'"
    );
}

#[test]
fn type_query_combines_with_free_text_and_the_gallery_category() {
    let modules = typed_modules();
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "type:isf sea"
        )),
        vec!["Deep Sea"]
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::All,
            "sea type:isf"
        )),
        vec!["Deep Sea"],
        "the type token may appear before or after the free-text terms"
    );
    assert_eq!(
        names(&filter_gallery_modules(
            &modules,
            GalleryCategory::ThreeDScenes,
            "type:isf"
        )),
        vec!["Tunnel", "Deep Sea"]
    );
    assert!(
        filter_gallery_modules(&modules, GalleryCategory::Effects, "type:isf sea").is_empty(),
        "category filtering still applies on top of a type query"
    );
}

#[test]
fn unknown_type_matches_nothing() {
    let modules = typed_modules();
    assert!(
        filter_gallery_modules(&modules, GalleryCategory::All, "type:bogus").is_empty(),
        "an unsupported type token must not fall back to an unfiltered list"
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
