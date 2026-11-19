//! Version-keyed rename/removal migrations for the default Scenes.
//!
//! The default Scene set ships with [`crate::default_scenes::DEFAULTS_VERSION`]
//! (`default-scenes.js:39`). Seeding
//! ([`crate::default_scenes::get_scenes_to_seed`]) only appends names that are
//! new *and* neither already seeded nor already present, so it cannot express
//! two edits that a version bump may make to Scenes an install already holds:
//!
//! - a default Scene is **renamed** (the library would keep the stale name and
//!   gain nothing, or — if the rename also shipped a new definition — end up
//!   with two Scenes for one look), and
//! - a default Scene is **retired** (the old name would linger forever).
//!
//! This module is the data-driven table and the pure logic for those edits:
//!
//! - [`DEFAULT_SCENE_MIGRATIONS`] — one [`DefaultSceneMigrationRule`] per
//!   defaults-version bump that renamed or retired seeded Scenes.
//! - [`get_default_scene_migrations`] — aggregates the rules in `from..=to`.
//! - [`apply_default_scene_migrations`] — applies the aggregate to a Scene list
//!   and the `seededDefaultScenes` list, reporting what happened.
//! - [`ensure_default_scene_migration`] — the call the store makes before
//!   seeding: applies the migration and returns the definitions that still
//!   need to be built for the new version.
//!
//! ## Adding a rule
//!
//! When `default-scenes.js` renames or removes a default Scene, add the pair /
//! name to the rule for the version that ships the change (create the rule if
//! it is the first edit of that bump) and keep the table in ascending `version`
//! order. A rule only applies to installs moving across its version, exactly
//! once. The `the_production_table_is_consistent` test guards the invariants:
//! the old name must be gone from the data, the new name must exist.
//!
//! ## The production table
//!
//! The historical edits in the JS file are **not** version-keyed:
//! `RENAMED_DEFAULT_SCENES` (`default-scenes.js:60-100`) and
//! `REMOVED_DEFAULT_SCENES` (`default-scenes.js:49-58`) are applied to *any*
//! install below `DEFAULTS_VERSION` by `getDefaultScenesToReplace` /
//! `getDefaultScenesToRemove`, which remain the ported path for them
//! ([`crate::default_scenes::get_default_scenes_to_replace`],
//! [`crate::default_scenes::get_default_scenes_to_remove`]). Keying those rows
//! here would invent which bump each rename shipped in — and re-remove Scenes
//! the in-place refresh already handles.
//!
//! Version 18 is the first bump that ships a version-keyed removal: it retires
//! `Strobe Riot`, which is recorded here so the engine store drops the seeded
//! Scene on upgrade. Re-running the rule is inert because the name is gone
//! from both the Scene list and `seeded_names`.
//!
//! Version 19 renames `Prism Tunnel` to `Long Red Tunnel`: the Scene was
//! retuned in the maintainer's personal library and is replaced in the data
//! (the Background row, Auto Levels, embedded draw curve). The rename keeps
//! the seeded Scene's id and payload — for the maintainer that is the retuned
//! Scene itself; for a stock v18 install the old sparse definition keeps its
//! id under the new name, and fresh installs receive the new definition.
//!
//! Version 20 removes the `Point10 Rolling Fire`, `Point10 Ball Of Fire`,
//! `Point10 DVD Room`, `Point10 Chromatic Line` and `Point10 Smoke Room`
//! modules from the default Scenes. The first three are the heavy
//! volumetric scenes (~432 MB of 3D textures per instance); the other two
//! ship nothing an install needs. All five modules stay registered and
//! usable in user Scenes; the defaults simply never ship them.
//!
//! The surviving Scenes lose the modules in place (refresh-only, handled
//! by [`crate::default_scenes::REFRESHED_DEFAULT_SCENES`]); the ten whose
//! removal would leave nothing worth shipping (`Rolling Fire Bloom`,
//! `DVD Nightmare`, `Terminal Bloom`, `DVD Afterglow`, `Etched Aurora`,
//! `Lone Chrome`, `Smoke Room`, `Mirror Chamber`, `Glitch Bloom Cathedral`
//! and `Circuit Ritual`) are retired here — a seeded install drops them on
//! upgrade.
//!
//! Version 13 is the first bump whose edits are refresh-only (no Scene was
//! renamed or retired): `Halftone Harbor` loses CMYK Halftone and `MBOX Light
//! Tunnel` fixes its `MBOX3` hue, both handled by
//! [`crate::default_scenes::get_default_scenes_to_replace`] through
//! [`crate::default_scenes::REFRESHED_DEFAULT_SCENES`]. It adds no row here.
//! The `the_previous_version_bump_refreshes_the_edited_scenes_without_a_rename_rule`
//! test pins that split.
//!
//! Version 19 is the first bump that both refreshes **nothing** in place and
//! ships a version-keyed rename: `Prism Tunnel` becomes `Long Red Tunnel`. The
//! seeded Scene keeps its id and payload; only the name (and the seeded
//! marker) changes.
//!
//! ## Semantics
//!
//! A "Scene" here is a serialised Scene document ([`serde_json::Value`]); the
//! engine keeps Scenes as JSON documents (`ScenesState::scenes` in
//! `modv-app`), so the helpers take `&mut Vec<Value>`.
//!
//! - Names are matched with [`crate::default_scenes::scene_key`] (trim +
//!   lower-case), like every other seeding/migration helper.
//! - Only names listed in `seeded_names` are migrated. A user-created Scene
//!   that happens to share a default's name is never touched (and a user Scene
//!   whose name is not in `seeded_names` blocks a rename collision, see below).
//! - A **rename** rewrites the Scene's `name` in place, keeps its `id`, and
//!   replaces the old entry in `seeded_names`. The JS
//!   `getDefaultScenesToReplace` implements a rename as remove + re-add, so the
//!   migrated Scene receives a **fresh id** there. Keeping the id here
//!   preserves `currentSceneId`, the Scene's timing override and playlist
//!   references across the bump; `seeded_names` still maps old → name so
//!   seeding cannot add a duplicate.
//! - A **removal** deletes the first seeded Scene carrying the retired name
//!   (the JS removes one Scene per rule, by id) and drops the name from
//!   `seeded_names`. Another same-named Scene — a seeded duplicate, a user
//!   Scene, or an import — is kept. A removed name that still exists in the
//!   current defaults would be re-seeded, so a rule must only retire names that
//!   left the data (guarded by the consistency test).
//! - A rename whose target name is already carried by another Scene is
//!   **skipped** (reported in [`MigrationReport::skipped`]), never merged or
//!   overwritten.
//! - Re-running the same migration is a no-op: the old name is gone, so the
//!   second pass reports it as skipped and leaves the library and
//!   `seeded_names` unchanged.
//!
//! The rules ported here correspond to no specific JS function: they are the
//! version-keyed complement of `default-scenes.js`'s unversioned tables.
//! Line numbers refer to the tree at commit `8a512b6e` (branch `rust-hybrid`).

use std::collections::HashSet;

use serde_json::Value;

use crate::default_scenes::{SceneDocument, get_scenes_to_seed, scene_key};

/// One defaults-version bump's rename/removal edits.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultSceneMigrationRule {
    /// The [`crate::default_scenes::DEFAULTS_VERSION`] bump this rule ships
    /// with. Applied to installs moving from `< version` to `>= version`.
    pub version: i64,
    /// `(old name, new name)` pairs for Scenes renamed in this bump.
    pub renames: &'static [(&'static str, &'static str)],
    /// Names retired from the defaults in this bump.
    pub removed: &'static [&'static str],
}

/// The version-keyed rename/removal table.
///
/// Version 18 retires `Strobe Riot`; version 19 renames `Prism Tunnel` to
/// `Long Red Tunnel`; version 20 removes five modules from the defaults and
/// retires the ten Scenes that were built around one of them. Add one row per
/// defaults-version bump that renames or retires a seeded Scene, in
/// ascending `version` order.
pub const DEFAULT_SCENE_MIGRATIONS: &[DefaultSceneMigrationRule] = &[
    DefaultSceneMigrationRule {
        version: 18,
        renames: &[],
        removed: &["Strobe Riot"],
    },
    DefaultSceneMigrationRule {
        version: 19,
        renames: &[("Prism Tunnel", "Long Red Tunnel")],
        removed: &[],
    },
    DefaultSceneMigrationRule {
        version: 20,
        renames: &[],
        removed: &[
            "Rolling Fire Bloom",
            "DVD Nightmare",
            "Terminal Bloom",
            "DVD Afterglow",
            "Etched Aurora",
            "Lone Chrome",
            "Smoke Room",
            "Mirror Chamber",
            "Glitch Bloom Cathedral",
            "Circuit Ritual",
        ],
    },
];

/// The aggregated edits for a version span.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefaultSceneMigration {
    /// `(old name, new name)` pairs, in application order.
    pub rename: Vec<(String, String)>,
    /// Retired names, in application order.
    pub remove: Vec<String>,
}

impl DefaultSceneMigration {
    /// Whether there is nothing to migrate.
    pub fn is_empty(&self) -> bool {
        self.rename.is_empty() && self.remove.is_empty()
    }
}

/// What [`apply_default_scene_migrations`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Scenes renamed, as `(old name, new name)`.
    pub renamed: Vec<(String, String)>,
    /// Seeded Scenes removed (each name once).
    pub removed: Vec<String>,
    /// Rules that did not apply, keyed by their old/retired name: no seeded
    /// Scene carried it, or the rename target was already taken.
    pub skipped: Vec<String>,
}

/// What [`ensure_default_scene_migration`] did and what the caller must seed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DefaultSceneMigrationPlan {
    /// The aggregated edits for the version span.
    pub migration: DefaultSceneMigration,
    /// Which of them applied.
    pub report: MigrationReport,
    /// Definitions introduced after `from_version` that are still missing and
    /// must be built and marked seeded by the caller — the same list
    /// [`crate::default_scenes::get_scenes_to_seed`] would return *after* the
    /// migration.
    pub seeds: Vec<&'static SceneDocument>,
}

/// Aggregate every [`DEFAULT_SCENE_MIGRATIONS`] rule in `from_version..=to_version`.
///
/// Empty when `to_version <= from_version`, so an install that is already on
/// (or ahead of) the target never re-applies history.
pub fn get_default_scene_migrations(from_version: i64, to_version: i64) -> DefaultSceneMigration {
    get_default_scene_migrations_with(DEFAULT_SCENE_MIGRATIONS, from_version, to_version)
}

/// [`get_default_scene_migrations`] with an explicit rule table (tests, or a
/// caller that composes rules from elsewhere).
pub fn get_default_scene_migrations_with(
    rules: &[DefaultSceneMigrationRule],
    from_version: i64,
    to_version: i64,
) -> DefaultSceneMigration {
    let mut result = DefaultSceneMigration::default();

    if to_version <= from_version {
        return result;
    }

    let mut seen_renames: HashSet<String> = HashSet::new();
    let mut seen_removals: HashSet<String> = HashSet::new();

    for rule in rules {
        if rule.version <= from_version || rule.version > to_version {
            continue;
        }

        for (old_name, new_name) in rule.renames {
            if seen_renames.insert(scene_key(old_name)) {
                result
                    .rename
                    .push(((*old_name).to_string(), (*new_name).to_string()));
            }
        }

        for name in rule.removed {
            if seen_removals.insert(scene_key(name)) {
                result.remove.push((*name).to_string());
            }
        }
    }

    result
}

/// Apply `migration` to `scenes` and `seeded_names`, reporting the outcome.
///
/// Renames run before removals (so a bump may rename into a name it retires
/// later, though the consistency test discourages that). Only names listed in
/// `seeded_names` are touched; see the module docs for the id/collision rules.
// The removal path uses `Vec::retain` and the store passes its Scene list, so
// the public API keeps `&mut Vec` rather than a slice.
#[allow(clippy::ptr_arg)]
pub fn apply_default_scene_migrations(
    scenes: &mut Vec<Value>,
    seeded_names: &mut Vec<String>,
    migration: &DefaultSceneMigration,
) -> MigrationReport {
    let mut report = MigrationReport::default();

    for (old_name, new_name) in &migration.rename {
        apply_rename(scenes, seeded_names, old_name, new_name, &mut report);
    }

    for name in &migration.remove {
        apply_removal(scenes, seeded_names, name, &mut report);
    }

    report
}

/// The migration + seeding plan for one version bump.
///
/// Run this *before* seeding: renames/removals are applied first (so a renamed
/// Scene is not seeded again under its new name and a retired Scene is not
/// resurrected), then [`crate::default_scenes::get_scenes_to_seed`] is computed
/// from the migrated `seeded_names`/`existing names`. The caller builds
/// [`DefaultSceneMigrationPlan::seeds`] and marks their names seeded; passing
/// `to_version = DEFAULTS_VERSION` makes an install on `DEFAULTS_VERSION - 1`
/// converge on the fresh-install set exactly once.
pub fn ensure_default_scene_migration(
    from_version: i64,
    to_version: i64,
    scenes: &mut Vec<Value>,
    seeded_names: &mut Vec<String>,
) -> DefaultSceneMigrationPlan {
    ensure_default_scene_migrations_with(
        DEFAULT_SCENE_MIGRATIONS,
        from_version,
        to_version,
        scenes,
        seeded_names,
    )
}

/// [`ensure_default_scene_migration`] with an explicit rule table.
pub fn ensure_default_scene_migrations_with(
    rules: &[DefaultSceneMigrationRule],
    from_version: i64,
    to_version: i64,
    scenes: &mut Vec<Value>,
    seeded_names: &mut Vec<String>,
) -> DefaultSceneMigrationPlan {
    let migration = get_default_scene_migrations_with(rules, from_version, to_version);
    let report = apply_default_scene_migrations(scenes, seeded_names, &migration);
    let existing_names: Vec<String> = scenes
        .iter()
        .filter_map(|scene| scene.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let seeds = get_scenes_to_seed(from_version, seeded_names, &existing_names);

    DefaultSceneMigrationPlan {
        migration,
        report,
        seeds,
    }
}

/// A Scene document's `name`, if it is a string.
fn scene_name(scene: &Value) -> Option<&str> {
    scene.get("name").and_then(Value::as_str)
}

/// Rename the first seeded Scene called `old_name` to `new_name`.
fn apply_rename(
    scenes: &mut [Value],
    seeded_names: &mut Vec<String>,
    old_name: &str,
    new_name: &str,
    report: &mut MigrationReport,
) {
    let old_key = scene_key(old_name);
    let new_key = scene_key(new_name);

    // A case/spacing-only edit has nothing to rewrite; name matching is
    // case-insensitive everywhere, so treat it as a no-op rather than a rule
    // failure.
    if old_key == new_key {
        return;
    }

    let Some(index) = seeded_scene_index(scenes, seeded_names, &old_key) else {
        report.skipped.push(old_name.to_string());
        return;
    };

    // Never merge into or overwrite a Scene that already owns the new name
    // (a user Scene, a previously renamed Scene, or a duplicate import).
    if scenes.iter().enumerate().any(|(other, scene)| {
        other != index && scene_name(scene).is_some_and(|name| scene_key(name) == new_key)
    }) {
        report.skipped.push(old_name.to_string());
        return;
    }

    let actual_old = scene_name(&scenes[index]).unwrap_or(old_name).to_string();

    if let Some(object) = scenes[index].as_object_mut() {
        object.insert("name".to_string(), Value::String(new_name.to_string()));
    } else {
        // Not a JSON object: leave it alone rather than replacing the payload.
        report.skipped.push(old_name.to_string());
        return;
    }

    rename_seeded_entry(seeded_names, &old_key, new_name);
    report.renamed.push((actual_old, new_name.to_string()));
}

/// Delete the first seeded Scene called `name` and stop treating the name as
/// seeded.
///
/// The JS removal path removes **one** Scene per rule — the id
/// `getDefaultScenesToRemove` reported — and then deletes the name from its
/// `seededKeys` set (`scenes.js:3028-3047`). Removing every same-named Scene
/// would delete an unrelated seeded duplicate as well. Dropping *all*
/// `seeded_names` entries for the name (not just the first) keeps a second run
/// from retiring the surviving duplicate: the JS never runs a removal twice for
/// the same bump, but the engine may re-apply a migration on a failed upgrade.
fn apply_removal(
    scenes: &mut Vec<Value>,
    seeded_names: &mut Vec<String>,
    name: &str,
    report: &mut MigrationReport,
) {
    let key = scene_key(name);
    let seeded: HashSet<String> = seeded_names.iter().map(|name| scene_key(name)).collect();

    if !seeded.contains(&key) {
        report.skipped.push(name.to_string());
        return;
    }

    // Only seeded Scenes are retired; a same-named user Scene is kept, and
    // exactly one duplicate is removed.
    let Some(index) = scenes
        .iter()
        .position(|scene| scene_name(scene).is_some_and(|scene| scene_key(scene) == key))
    else {
        report.skipped.push(name.to_string());
        return;
    };

    let actual = scene_name(&scenes[index]).unwrap_or(name).to_string();
    scenes.remove(index);

    seeded_names.retain(|seeded| scene_key(seeded) != key);
    report.removed.push(actual);
}

/// Index of the first Scene whose name matches `key` and that was seeded.
fn seeded_scene_index(scenes: &[Value], seeded_names: &[String], key: &str) -> Option<usize> {
    let seeded = seeded_names.iter().any(|name| scene_key(name) == key);

    if !seeded {
        return None;
    }

    scenes
        .iter()
        .position(|scene| scene_name(scene).is_some_and(|name| scene_key(name) == key))
}

/// Replace the `seeded_names` entry for `old_key` with `new_name`.
///
/// Duplicate old entries collapse to one; when the new name is already in
/// `seeded_names` (a Scene the user deleted after it was seeded) the old entry
/// is dropped instead of creating a second entry for the same name.
fn rename_seeded_entry(seeded_names: &mut Vec<String>, old_key: &str, new_name: &str) {
    let new_key = scene_key(new_name);
    let has_new = seeded_names.iter().any(|name| scene_key(name) == new_key);
    let mut replaced = false;

    seeded_names.retain_mut(|name| {
        if scene_key(name) != old_key {
            return true;
        }

        if has_new || replaced {
            return false;
        }

        *name = new_name.to_string();
        replaced = true;
        true
    });
}

#[cfg(test)]
mod tests {
    //! The mechanism is exercised with an explicit fixture table; the
    //! production table's version-18/19/20 rules are covered separately.
    use super::*;
    use crate::default_scenes::{
        DEFAULTS_VERSION, LEGACY_DEFAULTS_VERSION, get_default_definition,
        get_default_scene_names_up_to,
    };
    use serde_json::json;

    /// A bump at the current defaults version renames a Scene that exists in
    /// the data (under its historical name) and retires a name that left the
    /// data; the next bump chains a second rename.
    const FIXTURE_RULES: &[DefaultSceneMigrationRule] = &[
        DefaultSceneMigrationRule {
            version: DEFAULTS_VERSION,
            renames: &[("Old Mirror Chamber", "Mirror Chamber")],
            removed: &["Old Retired Scene"],
        },
        DefaultSceneMigrationRule {
            version: DEFAULTS_VERSION + 1,
            renames: &[("Mirror Chamber", "Reflected Chamber")],
            removed: &[],
        },
    ];

    fn scene(name: &str, id: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "minSeconds": 8,
            "maxSeconds": 20,
            "groups": [{"name": "Background", "modules": []}],
        })
    }

    fn names(scenes: &[Value]) -> Vec<String> {
        scenes
            .iter()
            .map(|scene| scene["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    fn migration(renames: &[(&str, &str)], removed: &[&str]) -> DefaultSceneMigration {
        DefaultSceneMigration {
            rename: renames
                .iter()
                .map(|(old, new)| ((*old).to_string(), (*new).to_string()))
                .collect(),
            remove: removed.iter().map(|name| (*name).to_string()).collect(),
        }
    }

    // ————————————————— version aggregation —————————————————

    #[test]
    fn aggregates_every_bump_in_the_range() {
        let aggregated = get_default_scene_migrations_with(
            FIXTURE_RULES,
            DEFAULTS_VERSION - 1,
            DEFAULTS_VERSION + 1,
        );

        assert_eq!(
            aggregated.rename,
            vec![
                (
                    "Old Mirror Chamber".to_string(),
                    "Mirror Chamber".to_string()
                ),
                (
                    "Mirror Chamber".to_string(),
                    "Reflected Chamber".to_string()
                ),
            ]
        );
        assert_eq!(aggregated.remove, vec!["Old Retired Scene".to_string()]);
    }

    #[test]
    fn excludes_bumps_at_or_before_the_from_version() {
        let aggregated = get_default_scene_migrations_with(
            FIXTURE_RULES,
            DEFAULTS_VERSION,
            DEFAULTS_VERSION + 1,
        );

        assert_eq!(
            aggregated.rename,
            vec![(
                "Mirror Chamber".to_string(),
                "Reflected Chamber".to_string()
            )]
        );
        assert!(aggregated.remove.is_empty());
    }

    #[test]
    fn returns_an_empty_migration_for_same_or_older_versions() {
        for (from, to) in [
            (DEFAULTS_VERSION, DEFAULTS_VERSION),
            (DEFAULTS_VERSION + 1, DEFAULTS_VERSION),
            (0, 0),
            (12, -1),
        ] {
            assert!(
                get_default_scene_migrations_with(FIXTURE_RULES, from, to).is_empty(),
                "{from}..{to}"
            );
        }

        assert!(get_default_scene_migrations(DEFAULTS_VERSION, DEFAULTS_VERSION).is_empty());
    }

    #[test]
    fn deduplicates_a_name_repeated_across_rules() {
        const REPEATED: &[DefaultSceneMigrationRule] = &[
            DefaultSceneMigrationRule {
                version: 8,
                renames: &[("Old", "New")],
                removed: &["Gone"],
            },
            DefaultSceneMigrationRule {
                version: 9,
                renames: &[(" old ", "New")],
                removed: &["gone"],
            },
        ];

        let aggregated = get_default_scene_migrations_with(REPEATED, 7, 9);

        assert_eq!(
            aggregated.rename,
            vec![("Old".to_string(), "New".to_string())]
        );
        assert_eq!(aggregated.remove, vec!["Gone".to_string()]);
    }

    #[test]
    fn the_production_table_is_consistent() {
        let mut last_version = LEGACY_DEFAULTS_VERSION;

        for rule in DEFAULT_SCENE_MIGRATIONS {
            assert!(
                rule.version >= last_version,
                "rules must be in ascending version order"
            );
            last_version = rule.version;

            for (old_name, new_name) in rule.renames {
                assert_ne!(scene_key(old_name), scene_key(new_name));
                assert!(
                    get_default_definition(old_name).is_none(),
                    "{old_name} was renamed but is still in the defaults"
                );
                assert!(
                    get_default_definition(new_name).is_some(),
                    "{new_name} is missing from the defaults"
                );
            }

            for name in rule.removed {
                assert!(
                    get_default_definition(name).is_none(),
                    "{name} was retired but is still in the defaults"
                );
            }
        }
    }

    // ————————————————— renames —————————————————

    #[test]
    fn renames_only_seeded_scenes() {
        let mut scenes = vec![
            scene("Old Mirror Chamber", "seeded-id"),
            scene("Smoke Room", "other-id"),
        ];
        let mut seeded = vec!["Old Mirror Chamber".to_string(), "Smoke Room".to_string()];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]),
        );

        assert_eq!(
            names(&scenes),
            vec!["Mirror Chamber".to_string(), "Smoke Room".to_string()]
        );
        assert_eq!(
            report.renamed,
            vec![(
                "Old Mirror Chamber".to_string(),
                "Mirror Chamber".to_string()
            )]
        );
        assert!(report.removed.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn leaves_a_user_scene_with_the_old_name_untouched() {
        let mut scenes = vec![scene("Old Mirror Chamber", "user-id")];
        let mut seeded = Vec::new();

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]),
        );

        assert_eq!(names(&scenes), vec!["Old Mirror Chamber".to_string()]);
        assert!(report.renamed.is_empty());
        assert_eq!(report.skipped, vec!["Old Mirror Chamber".to_string()]);
        assert!(seeded.is_empty());
    }

    #[test]
    fn a_rename_keeps_the_scene_id_and_payload() {
        let mut scenes = vec![scene("Old Mirror Chamber", "mirror-id")];
        let mut seeded = vec!["Old Mirror Chamber".to_string()];

        apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]),
        );

        let renamed = &scenes[0];

        assert_eq!(renamed["id"], json!("mirror-id"));
        assert_eq!(renamed["name"], json!("Mirror Chamber"));
        assert_eq!(renamed["minSeconds"], json!(8));
        assert_eq!(
            renamed["groups"],
            scene("Mirror Chamber", "mirror-id")["groups"]
        );
    }

    #[test]
    fn a_rename_updates_seeded_names_and_is_idempotent() {
        let mut scenes = vec![scene("Old Mirror Chamber", "mirror-id")];
        let mut seeded = vec!["Old Mirror Chamber".to_string(), "Smoke Room".to_string()];
        let migration = migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]);

        let first = apply_default_scene_migrations(&mut scenes, &mut seeded, &migration);

        assert_eq!(
            seeded,
            vec!["Mirror Chamber".to_string(), "Smoke Room".to_string()]
        );

        let before_scenes = scenes.clone();
        let before_seeded = seeded.clone();
        let second = apply_default_scene_migrations(&mut scenes, &mut seeded, &migration);

        assert!(first.renamed.len() == 1);
        assert!(second.renamed.is_empty());
        assert_eq!(second.skipped, vec!["Old Mirror Chamber".to_string()]);
        assert_eq!(scenes, before_scenes);
        assert_eq!(seeded, before_seeded);
    }

    #[test]
    fn collapses_duplicate_seeded_names_when_the_new_name_is_already_seeded() {
        let mut scenes = vec![scene("Old Mirror Chamber", "mirror-id")];
        let mut seeded = vec![
            "Old Mirror Chamber".to_string(),
            "  mirror chamber ".to_string(),
        ];

        apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]),
        );

        assert_eq!(seeded, vec!["  mirror chamber ".to_string()]);
    }

    #[test]
    fn skips_a_rename_whose_target_name_is_taken() {
        let mut scenes = vec![
            scene("Old Mirror Chamber", "seeded-id"),
            scene("Mirror Chamber", "user-id"),
        ];
        let mut seeded = vec!["Old Mirror Chamber".to_string()];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("Old Mirror Chamber", "Mirror Chamber")], &[]),
        );

        assert_eq!(
            names(&scenes),
            vec![
                "Old Mirror Chamber".to_string(),
                "Mirror Chamber".to_string()
            ]
        );
        assert_eq!(scenes[1]["id"], json!("user-id"));
        assert!(report.renamed.is_empty());
        assert_eq!(report.skipped, vec!["Old Mirror Chamber".to_string()]);
        assert_eq!(seeded, vec!["Old Mirror Chamber".to_string()]);
    }

    #[test]
    fn skips_a_case_only_rename_without_reporting_it() {
        let mut scenes = vec![scene("Mirror Chamber", "mirror-id")];
        let mut seeded = vec!["Mirror Chamber".to_string()];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[("mirror chamber", "Mirror Chamber")], &[]),
        );

        assert!(report.renamed.is_empty());
        assert!(report.skipped.is_empty());
        assert_eq!(names(&scenes), vec!["Mirror Chamber".to_string()]);
    }

    #[test]
    fn chains_renames_across_bumps_in_order() {
        let aggregated = get_default_scene_migrations_with(
            FIXTURE_RULES,
            DEFAULTS_VERSION - 1,
            DEFAULTS_VERSION + 1,
        );
        let mut scenes = vec![scene("Old Mirror Chamber", "mirror-id")];
        let mut seeded = vec!["Old Mirror Chamber".to_string()];

        let report = apply_default_scene_migrations(&mut scenes, &mut seeded, &aggregated);

        assert_eq!(names(&scenes), vec!["Reflected Chamber".to_string()]);
        assert_eq!(seeded, vec!["Reflected Chamber".to_string()]);
        assert_eq!(
            report.renamed,
            vec![
                (
                    "Old Mirror Chamber".to_string(),
                    "Mirror Chamber".to_string()
                ),
                (
                    "Mirror Chamber".to_string(),
                    "Reflected Chamber".to_string()
                ),
            ]
        );
    }

    // ————————————————— removals —————————————————

    #[test]
    fn removes_seeded_scenes_and_their_seeded_names() {
        let mut scenes = vec![
            scene("Old Retired Scene", "retired-id"),
            scene("Smoke Room", "smoke-id"),
        ];
        let mut seeded = vec!["Old Retired Scene".to_string(), "Smoke Room".to_string()];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[], &["Old Retired Scene"]),
        );

        assert_eq!(names(&scenes), vec!["Smoke Room".to_string()]);
        assert_eq!(seeded, vec!["Smoke Room".to_string()]);
        assert_eq!(report.removed, vec!["Old Retired Scene".to_string()]);
        assert!(report.renamed.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn keeps_a_user_scene_that_shares_a_retired_name() {
        let mut scenes = vec![scene("Old Retired Scene", "user-id")];
        let mut seeded = Vec::new();

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[], &["Old Retired Scene"]),
        );

        assert_eq!(names(&scenes), vec!["Old Retired Scene".to_string()]);
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped, vec!["Old Retired Scene".to_string()]);
    }

    #[test]
    fn removes_one_of_two_duplicate_seeded_scenes_and_keeps_the_other() {
        // The JS removal path removes **one** Scene (by id) per rule
        // (`scenes.js:3028-3047`) and splices one `seededDefaultScenes` entry.
        // A duplicate seeded Scene must not be collateral damage.
        let mut scenes = vec![
            scene("Old Retired Scene", "first-id"),
            scene("Old Retired Scene", "second-id"),
            scene("Smoke Room", "smoke-id"),
        ];
        let mut seeded = vec![
            "Old Retired Scene".to_string(),
            "Old Retired Scene".to_string(),
            "Smoke Room".to_string(),
        ];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[], &["Old Retired Scene"]),
        );

        // Exactly one duplicate is gone; the survivor keeps its payload.
        assert_eq!(
            scenes
                .iter()
                .map(|scene| scene["id"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["second-id", "smoke-id"]
        );
        assert_eq!(report.removed, vec!["Old Retired Scene".to_string()]);
        // The retired name is no longer seeded, so the survivor is not removed
        // by a re-run (see `a_removal_is_idempotent_with_duplicate_seeded_scenes`).
        assert_eq!(seeded, vec!["Smoke Room".to_string()]);
    }

    #[test]
    fn a_removal_is_idempotent_with_duplicate_seeded_scenes() {
        let mut scenes = vec![
            scene("Old Retired Scene", "first-id"),
            scene("Old Retired Scene", "second-id"),
        ];
        let mut seeded = vec![
            "Old Retired Scene".to_string(),
            "Old Retired Scene".to_string(),
        ];
        let migration = migration(&[], &["Old Retired Scene"]);

        let first = apply_default_scene_migrations(&mut scenes, &mut seeded, &migration);
        assert_eq!(first.removed, vec!["Old Retired Scene".to_string()]);
        assert_eq!(scenes.len(), 1);

        // Re-running the same migration must not eat the surviving duplicate.
        let second = apply_default_scene_migrations(&mut scenes, &mut seeded, &migration);
        assert!(second.removed.is_empty());
        assert_eq!(second.skipped, vec!["Old Retired Scene".to_string()]);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0]["id"], json!("second-id"));
        assert!(seeded.is_empty());
    }

    #[test]
    fn skips_removals_with_no_matching_scene() {
        let mut scenes = vec![scene("Smoke Room", "smoke-id")];
        let mut seeded = vec!["Smoke Room".to_string()];

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &migration(&[], &["Does Not Exist"]),
        );

        assert_eq!(scenes.len(), 1);
        assert_eq!(seeded, vec!["Smoke Room".to_string()]);
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped, vec!["Does Not Exist".to_string()]);
    }

    #[test]
    fn an_empty_migration_changes_nothing() {
        let mut scenes = vec![scene("Smoke Room", "smoke-id")];
        let mut seeded = vec!["Smoke Room".to_string()];
        let before = scenes.clone();

        let report = apply_default_scene_migrations(
            &mut scenes,
            &mut seeded,
            &DefaultSceneMigration::default(),
        );

        assert!(report == MigrationReport::default());
        assert_eq!(scenes, before);
        assert_eq!(seeded, vec!["Smoke Room".to_string()]);
    }

    // ————————————————— seeding integration —————————————————

    #[test]
    fn upgrading_from_the_previous_version_matches_a_fresh_install() {
        // A fixture rule consistent with the *current* production data: the
        // rename target is a Scene that survives in the defaults.
        const LOCAL_RULES: &[DefaultSceneMigrationRule] = &[DefaultSceneMigrationRule {
            version: DEFAULTS_VERSION,
            renames: &[("Old Geometric Tunnel", "Geometric Tunnel")],
            removed: &["Old Retired Scene"],
        }];

        let previous_version = DEFAULTS_VERSION - 1;
        let mut seeded: Vec<String> = get_default_scene_names_up_to(previous_version)
            .into_iter()
            .map(|name| {
                if name == "Geometric Tunnel" {
                    "Old Geometric Tunnel".to_string()
                } else {
                    name
                }
            })
            .collect();
        seeded.push("Old Retired Scene".to_string());

        let mut scenes: Vec<Value> = seeded
            .iter()
            .enumerate()
            .map(|(index, name)| scene(name, &format!("id-{index}")))
            .collect();
        let renamed_index = scenes
            .iter()
            .position(|scene| scene["name"] == json!("Old Geometric Tunnel"))
            .expect("historical Geometric Tunnel");

        let plan = ensure_default_scene_migrations_with(
            LOCAL_RULES,
            previous_version,
            DEFAULTS_VERSION,
            &mut scenes,
            &mut seeded,
        );

        assert_eq!(
            plan.report.renamed,
            vec![(
                "Old Geometric Tunnel".to_string(),
                "Geometric Tunnel".to_string()
            )]
        );
        assert_eq!(plan.report.removed, vec!["Old Retired Scene".to_string()]);
        assert_eq!(
            scenes[renamed_index]["id"],
            json!(format!("id-{renamed_index}"))
        );

        // The caller builds the seeds and marks them seeded; afterwards the
        // name set must equal a fresh install, with no duplicates.
        let mut upgraded = names(&scenes);
        upgraded.extend(plan.seeds.iter().map(|definition| definition.name.clone()));
        let mut upgraded_keys: Vec<String> = upgraded.iter().map(|name| scene_key(name)).collect();
        upgraded_keys.sort();

        let mut fresh: Vec<String> = get_default_scene_names_up_to(DEFAULTS_VERSION)
            .iter()
            .map(|name| scene_key(name))
            .collect();
        fresh.sort();

        assert_eq!(upgraded_keys, fresh);
        assert_eq!(
            upgraded_keys.iter().collect::<HashSet<_>>().len(),
            upgraded_keys.len()
        );
        assert_eq!(
            upgraded_keys.len(),
            crate::default_scenes::default_scenes().len()
        );
        assert!(
            !plan
                .seeds
                .iter()
                .any(|definition| { definition.introduced_version <= previous_version })
        );
    }

    #[test]
    fn re_running_the_upgrade_migration_seeds_and_changes_nothing() {
        // Already upgraded: the renamed Scene carries the new name and the
        // retired one is gone, so a second pass over the same span is inert.
        let mut scenes = vec![
            scene("Mirror Chamber", "mirror-id"),
            scene("Smoke Room", "smoke-id"),
        ];
        let mut seeded = vec!["Mirror Chamber".to_string(), "Smoke Room".to_string()];
        let before = scenes.clone();

        let plan = ensure_default_scene_migrations_with(
            FIXTURE_RULES,
            DEFAULTS_VERSION - 1,
            DEFAULTS_VERSION,
            &mut scenes,
            &mut seeded,
        );

        assert_eq!(
            plan.migration,
            migration(
                &[("Old Mirror Chamber", "Mirror Chamber")],
                &["Old Retired Scene"]
            )
        );
        assert!(plan.report.renamed.is_empty());
        assert!(plan.report.removed.is_empty());
        assert_eq!(plan.report.skipped.len(), 2);
        assert_eq!(scenes, before);
        assert_eq!(
            seeded,
            vec!["Mirror Chamber".to_string(), "Smoke Room".to_string()]
        );
        // Seeding is idempotent too: the Scene introduced in this bump was
        // never built here, so it is still offered (and the caller marks it
        // seeded), but no seeded name is duplicated.
        assert!(plan.seeds.iter().all(|definition| {
            !seeded
                .iter()
                .any(|name| scene_key(name) == scene_key(&definition.name))
        }));
    }

    #[test]
    fn the_production_path_retires_strobe_riot_for_installs_on_the_previous_version() {
        let mut scenes = vec![scene("Strobe Riot", "strobe-id")];
        let mut seeded = vec!["Strobe Riot".to_string()];

        // The v18 bump, pinned to its historical span.
        let plan = ensure_default_scene_migrations_with(
            DEFAULT_SCENE_MIGRATIONS,
            17,
            18,
            &mut scenes,
            &mut seeded,
        );

        assert_eq!(plan.migration, migration(&[], &["Strobe Riot"]));
        assert_eq!(plan.report.removed, vec!["Strobe Riot".to_string()]);
        assert!(plan.report.renamed.is_empty());
        assert!(plan.report.skipped.is_empty());
        assert!(scenes.is_empty(), "the seeded Scene was not removed");
        assert!(
            seeded
                .iter()
                .all(|name| scene_key(name) != scene_key("Strobe Riot")),
            "Strobe Riot is still marked seeded"
        );
    }

    #[test]
    fn the_production_path_keeps_a_custom_strobe_riot_scene() {
        let mut scenes = vec![scene("Strobe Riot", "user-id")];
        let mut seeded = vec!["Something Else".to_string()];

        let plan = ensure_default_scene_migrations_with(
            DEFAULT_SCENE_MIGRATIONS,
            17,
            18,
            &mut scenes,
            &mut seeded,
        );

        assert!(plan.report.removed.is_empty());
        assert_eq!(plan.report.skipped, vec!["Strobe Riot".to_string()]);
        assert_eq!(names(&scenes), vec!["Strobe Riot".to_string()]);
        assert_eq!(seeded, vec!["Something Else".to_string()]);
    }

    #[test]
    fn the_production_path_renames_prism_tunnel_for_installs_on_the_previous_version() {
        // Version 19 renames the seeded Scene in place (keeping its id, so the
        // active Scene and playlist references survive) and rewrites the
        // seeded marker; the definition change reaches fresh installs and the
        // rename audit in `the_production_table_is_consistent`.
        let mut scenes = vec![scene("Prism Tunnel", "prism-id")];
        let mut seeded = vec!["Prism Tunnel".to_string()];

        let plan = ensure_default_scene_migrations_with(
            DEFAULT_SCENE_MIGRATIONS,
            18,
            19,
            &mut scenes,
            &mut seeded,
        );

        assert_eq!(
            plan.migration,
            migration(&[("Prism Tunnel", "Long Red Tunnel")], &[])
        );
        assert_eq!(
            plan.report.renamed,
            vec![("Prism Tunnel".to_string(), "Long Red Tunnel".to_string())]
        );
        assert!(plan.report.removed.is_empty());
        assert!(plan.report.skipped.is_empty());
        assert_eq!(scenes[0]["name"], json!("Long Red Tunnel"));
        assert_eq!(scenes[0]["id"], json!("prism-id"));
        assert_eq!(seeded, vec!["Long Red Tunnel".to_string()]);
        // The renamed Scene is already present and marked seeded, so the
        // bump seeds nothing new for this install.
        assert!(plan.seeds.is_empty());
    }

    #[test]
    fn the_production_path_retires_the_version_20_scenes_for_installs_on_the_previous_version() {
        // Version 20 removes five modules from the default Scenes and retires
        // the ten that were built around one of them (the modules stay
        // registered for user Scenes). Seeded installs drop the retired
        // Scenes; a custom Scene sharing one of the names is kept.
        let retired = [
            "Rolling Fire Bloom",
            "DVD Nightmare",
            "Terminal Bloom",
            "DVD Afterglow",
            "Etched Aurora",
            "Lone Chrome",
            "Smoke Room",
            "Mirror Chamber",
            "Glitch Bloom Cathedral",
            "Circuit Ritual",
        ];

        for name in retired {
            let mut scenes = vec![scene(name, "seeded-id")];
            let mut seeded = vec![name.to_string()];

            let plan = ensure_default_scene_migrations_with(
                DEFAULT_SCENE_MIGRATIONS,
                19,
                20,
                &mut scenes,
                &mut seeded,
            );

            assert_eq!(plan.migration, migration(&[], &retired));
            assert_eq!(plan.report.removed, vec![name.to_string()]);
            assert!(
                !plan.report.skipped.iter().any(|skipped| skipped == name),
                "the seeded {name} Scene was skipped: {:?}",
                plan.report.skipped
            );
            assert!(scenes.is_empty(), "the seeded {name} Scene was not removed");
            assert_eq!(seeded, Vec::<String>::new());
        }
    }

    #[test]
    fn the_production_path_keeps_custom_scenes_sharing_the_retired_names() {
        for name in [
            "Rolling Fire Bloom",
            "DVD Nightmare",
            "Terminal Bloom",
            "DVD Afterglow",
            "Etched Aurora",
            "Lone Chrome",
            "Smoke Room",
            "Mirror Chamber",
            "Glitch Bloom Cathedral",
            "Circuit Ritual",
        ] {
            let mut scenes = vec![scene(name, "user-id")];
            let mut seeded = vec!["Something Else".to_string()];

            let plan = ensure_default_scene_migrations_with(
                DEFAULT_SCENE_MIGRATIONS,
                19,
                20,
                &mut scenes,
                &mut seeded,
            );

            assert!(plan.report.removed.is_empty(), "{name}");
            assert!(
                plan.report.skipped.iter().any(|skipped| skipped == name),
                "the custom {name} Scene was not kept: {:?}",
                plan.report.skipped
            );
            assert_eq!(names(&scenes), vec![name.to_string()]);
        }
    }

    #[test]
    fn the_production_path_seeds_long_red_tunnel_fresh_when_absent() {
        // An install that never held the Scene (or deleted it) receives the
        // retuned definition from the normal seed lane.
        let mut scenes = vec![];
        let mut seeded = vec![];

        let plan = ensure_default_scene_migrations_with(
            DEFAULT_SCENE_MIGRATIONS,
            18,
            19,
            &mut scenes,
            &mut seeded,
        );

        assert_eq!(plan.seeds.len(), 1);
        assert_eq!(plan.seeds[0].name, "Long Red Tunnel");
        assert_eq!(plan.seeds[0].introduced_version, 19);
    }

    #[test]
    fn the_production_path_keeps_a_custom_prism_tunnel_scene() {
        // A user Scene that shares the old name is never renamed.
        let mut scenes = vec![scene("Prism Tunnel", "user-id")];
        let mut seeded = vec!["Something Else".to_string()];

        let plan = ensure_default_scene_migrations_with(
            DEFAULT_SCENE_MIGRATIONS,
            18,
            19,
            &mut scenes,
            &mut seeded,
        );

        assert!(plan.report.renamed.is_empty());
        assert_eq!(plan.report.skipped, vec!["Prism Tunnel".to_string()]);
        assert_eq!(names(&scenes), vec!["Prism Tunnel".to_string()]);
    }

    #[test]
    fn the_previous_version_bump_refreshes_the_edited_scenes_without_a_rename_rule() {
        // Versions 13/14 only edit definitions in place — CMYK Halftone leaves
        // Halftone Harbor, MBOX3's hue is fixed and Point10 Old Movie becomes
        // Sepia Tone. Version 18 retires Strobe Riot without a rename, so the
        // version-keyed edits in the 12→18 span are its removal and nothing
        // else. Version 19's rename and version 20's retirements are covered
        // by their own tests. Installs on the previous version receive the
        // in-place edits through `get_default_scenes_to_replace`.
        let migration = get_default_scene_migrations(12, DEFAULTS_VERSION - 2);
        assert!(migration.rename.is_empty());
        assert_eq!(migration.remove, vec!["Strobe Riot".to_string()]);

        let names = ["Halftone Harbor", "MBOX Light Tunnel", "Boxy Arcade"];
        let scenes: Vec<Value> = names
            .iter()
            .map(|name| scene(name, &format!("{name}-id")))
            .collect();
        let seeded_names: Vec<String> = names.iter().map(|name| name.to_string()).collect();

        let plan = crate::default_scenes::get_default_scenes_to_replace(
            crate::default_scenes::DEFAULTS_VERSION - 1,
            &scenes,
            None,
            Some(&seeded_names),
        );
        let replaced: Vec<&str> = plan
            .replacements
            .iter()
            .map(|replacement| replacement.name.as_str())
            .collect();

        for name in names {
            assert!(replaced.contains(&name), "missing refresh {name}");
        }

        let halftone = plan
            .replacements
            .iter()
            .find(|replacement| replacement.name == "Halftone Harbor")
            .expect("Halftone Harbor refresh");
        assert_eq!(halftone.id, json!("Halftone Harbor-id"));
        assert!(
            !halftone
                .definition
                .module_names()
                .contains(&"CMYK Halftone"),
            "Halftone Harbor still uses CMYK Halftone"
        );
        assert!(halftone.definition.modules_in_group("Effects").is_empty());

        let mbox = plan
            .replacements
            .iter()
            .find(|replacement| replacement.name == "MBOX Light Tunnel")
            .expect("MBOX Light Tunnel refresh");
        assert_eq!(mbox.id, json!("MBOX Light Tunnel-id"));
        assert_eq!(
            mbox.definition
                .group("Background")
                .and_then(|group| group.modules.iter().find(|module| module.name == "MBOX3"))
                .and_then(|module| module.props.as_ref())
                .and_then(|props| props.get("dhue")),
            Some(&json!(0))
        );
        assert!(mbox.definition.link("MBOX3", "dhue").is_none());

        let boxy = plan
            .replacements
            .iter()
            .find(|replacement| replacement.name == "Boxy Arcade")
            .expect("Boxy Arcade refresh");
        assert_eq!(boxy.id, json!("Boxy Arcade-id"));
        let effects = boxy.definition.modules_in_group("Effects");
        assert!(
            effects.contains(&"Sepia Tone"),
            "Boxy Arcade was not refreshed: {effects:?}"
        );
        assert!(!effects.contains(&"Point10 Old Movie"));
    }
}
