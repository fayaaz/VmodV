//! Performance-grid cell editing: clearing, merging and copying a cell.
//!
//! Ported from `src/renderer/src/application/worker/scene-cell.js` (171
//! lines) and the `copySceneCellToScene` action in
//! `store/modules/scenes.js:3568-3735`. Every operation returns the Scene
//! payload the caller writes back; the store action wrapper (the Vuex commit)
//! stays in the UI.
//!
//! - [`clear_scene_cell_modules`] removes every Module from the Group saved in
//!   a cell and prunes their snapshots, returning the removed ids so the caller
//!   can also clean the live Group.
//! - [`merge_scene_cell`] writes an edited live Group back into a cell, merging
//!   the cell's module snapshots, inputs, input links, expressions and tweens
//!   with the rest of the Scene. Named rows look their Group up by name, so an
//!   appended Group can live anywhere; unnamed rows are positional and pad
//!   missing rows with `null` holes.
//! - [`copy_scene_cell`] copies a cell into another row of the same Scene or a
//!   row of another Scene: the Group verbatim, the Module snapshots it
//!   references, and only the inputs, input links, expression assignments and
//!   tweens reachable from them. Row names only pick the **source** Group; the
//!   write is positional (`groups[toRowIndex]`, like the JS), pads a shorter
//!   target with `null` holes and trims trailing holes.
//!

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value, json};

use crate::inputs::{data_type_input_keys, source_uses_tween_descriptor};
use crate::scene_rows::{
    get_row_group, get_row_group_index, get_row_name_index, is_row_cell_empty,
};
use crate::scenes_playlist::normalize_scene_document;

/// `getModuleIds` (`scene-cell.js:7-19`).
fn get_module_ids(group: &Value) -> Vec<String> {
    match group.get("modules") {
        Some(Value::Array(modules)) => modules
            .iter()
            .filter_map(|module| module.as_str().map(str::to_string))
            .collect(),
        Some(Value::Object(modules)) => modules.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Result of [`clear_scene_cell_modules`].
#[derive(Clone, Debug, PartialEq)]
pub struct ClearedSceneCell {
    /// The Scene's `groups` array with the cell's `modules` emptied.
    pub groups: Vec<Value>,
    /// The Scene's `modules` map without the removed snapshots.
    pub modules: Map<String, Value>,
    /// The Module ids that were removed from the cell.
    pub module_ids: Vec<String>,
}

/// `clone` (`scene-cell.js:3-5`): a `JSON.parse(JSON.stringify(...))` copy.
fn clone_value(value: &Value) -> Value {
    value.clone()
}

/// `clearSceneCellModules` (`scene-cell.js:76-101`).
///
/// Returns `None` when the cell has no Group or the Group has no Modules, so
/// the caller can skip the update. The Scene document is not mutated.
pub fn clear_scene_cell_modules(
    scene: &Value,
    row_index: usize,
    row_names: &[String],
) -> Option<ClearedSceneCell> {
    let source_groups = scene
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let index = get_row_group_index(scene, row_index, row_names)?;
    let group = source_groups.get(index)?;
    let module_ids = get_module_ids(group);

    if module_ids.is_empty() {
        return None;
    }

    let mut groups = source_groups
        .iter()
        .map(clone_value)
        .collect::<Vec<Value>>();
    groups[index]["modules"] = json!([]);

    let mut modules = scene
        .get("modules")
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default();

    for module_id in &module_ids {
        modules.remove(module_id);
    }

    Some(ClearedSceneCell {
        groups,
        modules,
        module_ids,
    })
}

/// Deep-copies a JSON object map.
fn clone_value_map(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// `pruneUnreferencedModules` (`scene-cell.js:21-35`).
fn prune_unreferenced_modules(groups: &[Value], modules: &mut Map<String, Value>) {
    let mut referenced: Vec<String> = Vec::new();

    for group in groups {
        for module_id in get_module_ids(group) {
            if !referenced.contains(&module_id) {
                referenced.push(module_id);
            }
        }
    }

    modules.retain(|module_id, _| referenced.contains(module_id));
}

/// `mergeTweens` (`scene-cell.js:37-69`): merge by tween id, keeping the
/// existing container shape (array vs object).
fn merge_tweens(existing: Option<&Value>, incoming: Option<&Value>) -> Value {
    let as_object = matches!(existing, Some(Value::Object(_)));
    let mut map: Map<String, Value> = Map::new();

    let mut add = |tweens: Option<&Value>| match tweens {
        Some(Value::Array(tweens)) => {
            for tween in tweens {
                if let Some(id) = tween.get("id").filter(|id| !id.is_null()) {
                    map.insert(id_key(id), tween.clone());
                }
            }
        }
        Some(Value::Object(tweens)) => {
            for (id, tween) in tweens {
                if tween.is_null() {
                    continue;
                }

                let key = tween
                    .get("id")
                    .filter(|id| !id.is_null())
                    .map(id_key)
                    .unwrap_or_else(|| id.clone());
                map.insert(key, tween.clone());
            }
        }
        _ => {}
    };

    add(existing);
    add(incoming);

    if as_object {
        Value::Object(map)
    } else {
        Value::Array(map.into_values().collect())
    }
}

/// A JS object key for a tween id: strings verbatim, anything else its JSON.
fn id_key(id: &Value) -> String {
    match id {
        Value::String(id) => id.clone(),
        other => other.to_string(),
    }
}

/// `mergeSceneCell` (`scene-cell.js:107-171`).
///
/// `cell` is the live snapshot: `{ group, modules, inputs, expressions, tweens }`.
/// Returns the merged Scene fields as an object, or `None` when the cell has no
/// Group.
pub fn merge_scene_cell(
    scene: &Value,
    row_index: usize,
    row_names: &[String],
    cell: &Value,
) -> Option<Value> {
    let group = cell.get("group").filter(|group| !group.is_null())?;

    let source_groups = scene
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut groups: Vec<Value> = source_groups.iter().map(clone_value).collect();
    let group = clone_value(group);

    match get_row_group_index(scene, row_index, row_names) {
        Some(index) => groups[index] = group,
        None => {
            if get_row_name_index(row_names, row_index).is_empty() {
                // Unnamed rows are positional: keep the cell on its row by
                // padding any missing rows with holes, exactly like a
                // Ctrl+drag copy does.
                if groups.len() < row_index {
                    groups.resize(row_index, Value::Null);
                }

                if groups.len() == row_index {
                    groups.push(group);
                } else {
                    groups[row_index] = group;
                }
            } else {
                // Named rows look their Group up by name, so it can live
                // anywhere.
                groups.push(group);
            }
        }
    }

    let mut modules = scene
        .get("modules")
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default();

    if let Some(cell_modules) = cell.get("modules").and_then(Value::as_object) {
        for (module_id, module) in cell_modules {
            modules.insert(module_id.clone(), module.clone());
        }
    }

    prune_unreferenced_modules(&groups, &mut modules);

    let base_inputs = scene
        .get("inputs")
        .filter(|inputs| inputs.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let cell_inputs = cell
        .get("inputs")
        .filter(|inputs| inputs.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));

    let mut inputs = base_inputs.as_object().cloned().unwrap_or_default();
    if !matches!(
        inputs.get("focusedInput"),
        Some(focused) if !focused.is_null()
    ) {
        inputs.insert(
            "focusedInput".to_string(),
            json!({"id": null, "title": null}),
        );
    }
    merge_object_key(&mut inputs, "inputs", &base_inputs, &cell_inputs);
    merge_object_key(&mut inputs, "inputLinks", &base_inputs, &cell_inputs);

    let base_expressions = scene
        .get("expressions")
        .filter(|expressions| expressions.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let cell_expressions = cell
        .get("expressions")
        .filter(|expressions| expressions.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));

    let mut expressions = base_expressions.as_object().cloned().unwrap_or_default();
    let base_assignments = base_expressions
        .get("assignments")
        .and_then(Value::as_object);
    let cell_assignments = cell_expressions
        .get("assignments")
        .and_then(Value::as_object);
    let mut assignments = base_assignments.map(clone_value_map).unwrap_or_default();

    if let Some(cell_assignments) = cell_assignments {
        for (id, assignment) in cell_assignments {
            assignments.insert(id.clone(), assignment.clone());
        }
    }

    expressions.insert("assignments".to_string(), Value::Object(assignments));

    Some(json!({
        "groups": groups,
        "modules": Value::Object(modules),
        "inputs": Value::Object(inputs),
        "expressions": Value::Object(expressions),
        "tweens": merge_tweens(scene.get("tweens"), cell.get("tweens")),
    }))
}

/// Merge one nested object key (`inputs.inputs`, `inputs.inputLinks`) from the
/// Scene and the cell into `target`.
fn merge_object_key(target: &mut Map<String, Value>, key: &str, base: &Value, cell: &Value) {
    let mut merged = base
        .get(key)
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default();

    if let Some(cell_values) = cell.get(key).and_then(Value::as_object) {
        for (id, value) in cell_values {
            merged.insert(id.clone(), value.clone());
        }
    }

    target.insert(key.to_string(), Value::Object(merged));
}

/// `GROUP_INPUT_NAMES` (`scenes.js:133-140`): the Group's own saved inputs,
/// stored on the Group as `<name>InputId`.
const GROUP_INPUT_NAMES: [&str; 6] = [
    "alpha",
    "enabled",
    "clearing",
    "inherit",
    "compositeOperation",
    "pipeline",
];

/// `MODULE_META_INPUT_NAMES` (`scenes.js:142`): the inputs a Module keeps in
/// its `meta` as `<name>InputId`.
const MODULE_META_INPUT_NAMES: [&str; 3] = ["alpha", "enabled", "compositeOperation"];

/// JS truthiness for a JSON value.
///
/// Only the falsy cases that matter here: `null`, `""`, `0` and `false`.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// The object key for a truthy id, or `None` for a falsy one.
fn truthy_key(value: &Value) -> Option<String> {
    is_truthy(value).then(|| id_key(value))
}

/// Adds an id once, keeping the JS `Set` insertion order.
fn push_unique(ids: &mut Vec<String>, id: String) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// `addModuleInputIds` (`scenes.js:211-240`): the Module's meta input ids, its
/// `$props` ids and each prop's data-type sub-inputs.
///
/// The sub-input keys come from [`crate::inputs::data_type_input_keys`], the
/// compiled-in `dataTypes` table.
fn add_module_input_ids(module: &Value, input_ids: &mut Vec<String>) {
    for name in MODULE_META_INPUT_NAMES {
        let input_id = module
            .get("meta")
            .and_then(|meta| meta.get(format!("{name}InputId")))
            .and_then(truthy_key);

        if let Some(input_id) = input_id {
            push_unique(input_ids, input_id);
        }
    }

    let Some(props) = module.get("$props").and_then(Value::as_object) else {
        return;
    };

    for prop in props.values() {
        let Some(prop_id) = prop.get("id").and_then(truthy_key) else {
            continue;
        };

        push_unique(input_ids, prop_id.clone());

        let type_name = prop.get("type").and_then(Value::as_str).unwrap_or_default();

        for key in crate::inputs::data_type_input_keys(type_name) {
            push_unique(input_ids, format!("{prop_id}-{key}"));
        }
    }
}

/// `getSceneInputLinks` (`scenes.js:307-313`): the Scene's input links, from
/// the nested `inputs.inputLinks` when truthy, else the legacy top level.
fn scene_input_links(scene: &Value) -> Option<&Map<String, Value>> {
    if let Some(links) = scene
        .get("inputs")
        .and_then(|inputs| inputs.get("inputLinks"))
        .filter(|links| is_truthy(links))
    {
        return links.as_object();
    }

    scene.get("inputLinks").and_then(Value::as_object)
}

/// `getSceneTweenEntries` (`scenes.js:315-325`): tween id → tween pairs from
/// either the array or the object container.
fn scene_tween_entries(scene: &Value) -> Vec<(String, Value)> {
    match scene.get("tweens") {
        Some(Value::Array(tweens)) => tweens
            .iter()
            .filter_map(|tween| {
                tween
                    .get("id")
                    .and_then(truthy_key)
                    .map(|id| (id, tween.clone()))
            })
            .collect(),
        Some(Value::Object(tweens)) => tweens
            .iter()
            .map(|(id, tween)| (id.clone(), tween.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// `Map#set`: overwrites in place, otherwise appends (insertion order is
/// preserved so the tween container round-trips like the JS `Map`).
fn map_set(map: &mut Vec<(String, Value)>, key: String, value: Value) {
    match map.iter_mut().find(|(existing, _)| *existing == key) {
        Some((_, slot)) => *slot = value,
        None => map.push((key, value)),
    }
}

/// `Map#get`.
fn map_get<'a>(map: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    map.iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value)
}

/// The tween descriptor an input link resolves to: by input id first, then by
/// the link's own id (`scenes.js:3669-3705`).
fn copied_tween<'a>(
    tweens: &'a [(String, Value)],
    link: &Value,
    input_id: &str,
) -> Option<&'a Value> {
    map_get(tweens, input_id).or_else(|| {
        link.get("id")
            .and_then(truthy_key)
            .and_then(|id| map_get(tweens, &id))
    })
}

/// A cloned JSON object, or an empty map for anything else (`|| {}`).
fn object_or_empty(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default()
}

/// A cloned nested object key, or an empty map.
fn nested_object(map: &Map<String, Value>, key: &str) -> Map<String, Value> {
    object_or_empty(map.get(key))
}

/// `{...target, ...source}` for JSON object maps.
fn extend_map(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

/// The tween container the target Scene uses (`toScene.tweens || {}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TweenContainer {
    /// A `tweens` array.
    Array,
    /// A `tweens` object (also the shape a missing/falsy container becomes).
    Object,
    /// A truthy non-container value: the JS never assigns `data.tweens`.
    Omitted,
}

/// A process-unique id for a copy, mirroring
/// [`crate::scenes_playlist::generate_scene_id`]. Only uniqueness within this
/// engine matters.
fn fresh_copy_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{prefix}-{now:x}-{sequence:x}")
}

/// The fresh id minted for an old one, if any.
fn lookup_fresh<'a>(map: &'a [(String, String)], id: &str) -> Option<&'a str> {
    map.iter()
        .find(|(existing, _)| existing == id)
        .map(|(_, fresh)| fresh.as_str())
}

/// The old id with its fresh replacement, or the old id when it is not being
/// remapped.
fn remap_id(map: &[(String, String)], id: &str) -> String {
    lookup_fresh(map, id)
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

/// Mints a fresh input id once per old id (so every reference agrees).
fn fresh_input_id(map: &mut Vec<(String, String)>, input_id: &str) -> String {
    if let Some(fresh) = lookup_fresh(map, input_id) {
        return fresh.to_string();
    }

    let fresh = fresh_copy_id("input");
    map.push((input_id.to_string(), fresh.clone()));
    fresh
}

/// Rewrites a truthy id field to its fresh replacement, when mapped.
fn remap_id_field(object: &mut Map<String, Value>, key: &str, map: &[(String, String)]) {
    if let Some(fresh) = object
        .get(key)
        .and_then(truthy_key)
        .and_then(|id| lookup_fresh(map, &id).map(str::to_string))
    {
        object.insert(key.to_string(), json!(fresh));
    }
}

/// Rewrites a copied Group's id, Module references (snapshotted ids only) and
/// `*InputId` fields to their fresh equivalents.
fn remap_group(
    group: &Value,
    module_ids: &[(String, String)],
    input_ids: &[(String, String)],
) -> Value {
    let mut copied = group.clone();
    let Some(object) = copied.as_object_mut() else {
        return copied;
    };

    object.insert("id".to_string(), json!(fresh_copy_id("group")));

    match object.get_mut("modules") {
        Some(Value::Array(modules)) => {
            for module in modules.iter_mut() {
                if let Some(module_id) = module.as_str()
                    && let Some(fresh) = lookup_fresh(module_ids, module_id)
                {
                    *module = json!(fresh);
                }
            }
        }
        Some(Value::Object(modules)) => {
            let remapped = modules
                .iter()
                .map(|(module_id, value)| {
                    let key = lookup_fresh(module_ids, module_id)
                        .map(str::to_string)
                        .unwrap_or_else(|| module_id.clone());
                    (key, value.clone())
                })
                .collect::<Map<String, Value>>();
            *modules = remapped;
        }
        _ => {}
    }

    for name in GROUP_INPUT_NAMES {
        remap_id_field(object, &format!("{name}InputId"), input_ids);
    }

    copied
}

/// Rewrites a copied Module snapshot's id, `meta.*InputId` and `$props.*.id`
/// to their fresh equivalents.
fn remap_module(module: &Value, fresh_id: &str, input_ids: &[(String, String)]) -> Value {
    let mut copied = module.clone();
    let Some(object) = copied.as_object_mut() else {
        return copied;
    };

    object.insert("id".to_string(), json!(fresh_id));
    if object.contains_key("$id") {
        object.insert("$id".to_string(), json!(fresh_id));
    }

    if let Some(meta) = object.get_mut("meta").and_then(Value::as_object_mut) {
        for name in MODULE_META_INPUT_NAMES {
            remap_id_field(meta, &format!("{name}InputId"), input_ids);
        }
    }

    if let Some(props) = object.get_mut("$props").and_then(Value::as_object_mut) {
        for prop in props.values_mut() {
            if let Some(prop) = prop.as_object_mut() {
                remap_id_field(prop, "id", input_ids);
            }
        }
    }

    copied
}

/// `copySceneCellToScene` (`scenes.js:3568-3735`): copy one Scene's cell into
/// a row of the same Scene or of another Scene.
///
/// `row_names` selects the **source** Group exactly like `getRowGroup` (named
/// rows match by name, unnamed rows are positional). The write is positional —
/// the JS plain-assigns `groups[toRowIndex]` — so a copied `Foreground` Group
/// lands on the target row that was dropped on, not on the target's own
/// `Foreground` row. Missing target rows are padded with `null` holes and
/// trailing holes are trimmed.
///
/// Returns the Scene fields to write back (`groups`, `modules`, `inputs`,
/// `expressions`, and `tweens` when the target has a tween container), or
/// `None` for the JS `return null` cases: the same Scene and the same row on
/// both sides, a Scene that cannot be normalised, or a missing/empty source
/// cell. Same Scene ids are compared as JS `===` on the id. Copying between
/// different rows of one Scene is allowed, but because both rows share the same
/// Scene maps the copy mints fresh ids for its whole closure (Group, Modules,
/// inputs, tween links/tweens and assignments) so the source row keeps its own.
pub fn copy_scene_cell(
    from_scene: &Value,
    to_scene: &Value,
    from_row_index: usize,
    to_row_index: usize,
    row_names: &[String],
) -> Option<Value> {
    let from_id = from_scene.get("id").and_then(truthy_key)?;
    let to_id = to_scene.get("id").and_then(truthy_key)?;

    if from_id == to_id && from_row_index == to_row_index {
        return None;
    }

    // Same Scene on both sides: the merged maps already hold the source row's
    // entries, so the copy must mint fresh ids instead of reusing them.
    let remap = from_id == to_id;

    let source_scene = normalize_scene_document(from_scene)?;
    let source_group = get_row_group(&source_scene, from_row_index, row_names)?;

    if is_row_cell_empty(&source_scene, from_row_index, row_names) {
        return None;
    }

    let no_modules = Map::new();
    let module_snapshots = source_scene
        .get("modules")
        .and_then(Value::as_object)
        .unwrap_or(&no_modules);

    // Only Modules with a snapshot ride along; a dangling id stays in the
    // copied Group (the Group is copied verbatim) but adds no snapshot.
    let copied_module_ids: Vec<String> = get_module_ids(source_group)
        .into_iter()
        .filter(|module_id| module_snapshots.get(module_id).is_some_and(is_truthy))
        .collect();

    // Saved input ids the copy carries with it, so inputs, links, expression
    // assignments and tweens ride along exactly.
    let mut copied_input_ids: Vec<String> = Vec::new();

    for name in GROUP_INPUT_NAMES {
        let input_id = source_group
            .get(format!("{name}InputId"))
            .and_then(truthy_key);

        if let Some(input_id) = input_id {
            push_unique(&mut copied_input_ids, input_id);
        }
    }

    for module_id in &copied_module_ids {
        if let Some(module) = module_snapshots.get(module_id) {
            add_module_input_ids(module, &mut copied_input_ids);
        }
    }

    // Same-Scene copies remap the copied closure onto fresh ids. Build the
    // Module and input maps before touching anything: every reference (Group,
    // Module meta/props, links, assignments) resolves through them.
    let mut module_id_map: Vec<(String, String)> = Vec::new();
    let mut input_id_map: Vec<(String, String)> = Vec::new();

    if remap {
        for module_id in &copied_module_ids {
            module_id_map.push((module_id.clone(), fresh_copy_id("module")));
        }

        for name in GROUP_INPUT_NAMES {
            let input_id = source_group
                .get(format!("{name}InputId"))
                .and_then(truthy_key);

            if let Some(input_id) = input_id {
                fresh_input_id(&mut input_id_map, &input_id);
            }
        }

        for module_id in &copied_module_ids {
            let Some(module) = module_snapshots.get(module_id) else {
                continue;
            };

            for name in MODULE_META_INPUT_NAMES {
                let input_id = module
                    .get("meta")
                    .and_then(|meta| meta.get(format!("{name}InputId")))
                    .and_then(truthy_key);

                if let Some(input_id) = input_id {
                    fresh_input_id(&mut input_id_map, &input_id);
                }
            }

            let Some(props) = module.get("$props").and_then(Value::as_object) else {
                continue;
            };

            for prop in props.values() {
                let Some(prop_id) = prop.get("id").and_then(truthy_key) else {
                    continue;
                };

                let fresh_prop_id = fresh_input_id(&mut input_id_map, &prop_id);
                let type_name = prop.get("type").and_then(Value::as_str).unwrap_or_default();

                // Sub-inputs keep the `-<key>` suffix and hang off the fresh
                // base id, so the composite family stays together.
                for key in data_type_input_keys(type_name) {
                    let composite_id = format!("{prop_id}-{key}");

                    if lookup_fresh(&input_id_map, &composite_id).is_none() {
                        input_id_map.push((composite_id, format!("{fresh_prop_id}-{key}")));
                    }
                }
            }
        }
    }

    // `data.modules = {...toScene.modules}`; copied snapshots win.
    let mut modules = to_scene
        .get("modules")
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default();

    for module_id in &copied_module_ids {
        if let Some(module) = module_snapshots.get(module_id) {
            if remap {
                let fresh_id = remap_id(&module_id_map, module_id);
                modules.insert(
                    fresh_id.clone(),
                    remap_module(module, &fresh_id, &input_id_map),
                );
            } else {
                modules.insert(module_id.clone(), module.clone());
            }
        }
    }

    // Row holes leave space in targets that have fewer Groups; trailing holes
    // are meaningless, so trim them.
    let mut groups: Vec<Value> = to_scene
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    while groups.len() <= to_row_index {
        groups.push(Value::Null);
    }

    groups[to_row_index] = if remap {
        remap_group(source_group, &module_id_map, &input_id_map)
    } else {
        source_group.clone()
    };

    while matches!(groups.last(), Some(Value::Null)) {
        groups.pop();
    }

    // `{...sourceInputs.inputs, ...toInputs.inputs}`, then the copied ids win.
    let source_inputs = object_or_empty(source_scene.get("inputs"));
    let to_inputs = object_or_empty(to_scene.get("inputs"));
    let source_link_map = scene_input_links(&source_scene)
        .map(clone_value_map)
        .unwrap_or_default();

    let mut inputs = nested_object(&source_inputs, "inputs");
    extend_map(&mut inputs, &nested_object(&to_inputs, "inputs"));

    // `{...toInputs.inputLinks, ...getSceneInputLinks(toScene)}`.
    let mut input_links = nested_object(&to_inputs, "inputLinks");

    if let Some(target_links) = scene_input_links(to_scene) {
        extend_map(&mut input_links, target_links);
    }

    for input_id in &copied_input_ids {
        if let Some(input) = source_inputs
            .get("inputs")
            .and_then(|inputs| inputs.get(input_id))
            .filter(|input| is_truthy(input))
        {
            inputs.insert(input_id.clone(), input.clone());

            // A same-Scene copy duplicates the input under a fresh id; the
            // original stays in place for the source row.
            if remap {
                inputs.insert(remap_id(&input_id_map, input_id), input.clone());
            }
        }

        if let Some(link) = source_link_map.get(input_id).filter(|link| is_truthy(link)) {
            input_links.insert(input_id.clone(), link.clone());
        }
    }

    let focused_input = to_inputs
        .get("focusedInput")
        .filter(|focused| is_truthy(focused))
        .cloned()
        .unwrap_or_else(|| json!({"id": null, "title": null}));

    // Tweens (id → tween) merged back into whatever shape the target uses.
    let to_tweens = to_scene.get("tweens");
    let tween_container = match to_tweens {
        Some(Value::Array(_)) => TweenContainer::Array,
        Some(Value::Object(_)) => TweenContainer::Object,
        Some(value) if is_truthy(value) => TweenContainer::Omitted,
        _ => TweenContainer::Object,
    };

    let mut tween_map: Vec<(String, Value)> = Vec::new();

    match to_tweens {
        Some(Value::Array(tweens)) => {
            for tween in tweens {
                if let Some(id) = tween.get("id").and_then(truthy_key) {
                    map_set(&mut tween_map, id, tween.clone());
                }
            }
        }
        Some(Value::Object(_)) => {
            for (id, tween) in scene_tween_entries(to_scene) {
                map_set(&mut tween_map, id, tween);
            }
        }
        _ => {}
    }

    let source_tweens = scene_tween_entries(&source_scene);
    // `Object.keys(inputLinks)` is snapshotted before the loop deletes links.
    let link_ids: Vec<String> = input_links.keys().cloned().collect();

    for input_id in link_ids {
        if !copied_input_ids.contains(&input_id) {
            continue;
        }

        let link = input_links
            .get(&input_id)
            .cloned()
            .unwrap_or_else(|| json!({}));

        if !source_uses_tween_descriptor(link.get("source").and_then(Value::as_str)) {
            continue;
        }

        let tween = copied_tween(&source_tweens, &link, &input_id);

        let resolved = tween.and_then(|tween| {
            let id = truthy_key(tween.get("id")?)?;
            Some((id, tween.clone()))
        });

        match resolved {
            Some((id, tween)) => map_set(&mut tween_map, id, tween),
            // A dangling tween link would leave the input without a value. A
            // same-Scene copy leaves the original link alone and simply does
            // not duplicate it.
            None => {
                if !remap {
                    input_links.remove(&input_id);
                }
            }
        }
    }

    // Same-Scene copies duplicate each reachable tween under a fresh id, point
    // the copied link at it, and re-key both onto the fresh input id.
    if remap {
        let mut fresh_tween_links: Vec<(String, String)> = Vec::new();
        let mut tween_id_map: Vec<(String, String)> = Vec::new();

        for input_id in &copied_input_ids {
            let Some(link) = input_links.get(input_id) else {
                continue;
            };

            if !source_uses_tween_descriptor(link.get("source").and_then(Value::as_str)) {
                continue;
            }

            let Some(tween) = copied_tween(&source_tweens, link, input_id) else {
                continue;
            };
            let Some(old_tween_id) = tween
                .get("id")
                .and_then(truthy_key)
                .or_else(|| link.get("id").and_then(truthy_key))
            else {
                continue;
            };

            let fresh_tween_id = match lookup_fresh(&tween_id_map, &old_tween_id) {
                Some(fresh) => fresh.to_string(),
                None => {
                    let fresh = fresh_copy_id("tween");
                    tween_id_map.push((old_tween_id.clone(), fresh.clone()));
                    fresh
                }
            };

            let mut duplicated = tween.clone();
            if let Some(object) = duplicated.as_object_mut() {
                object.insert("id".to_string(), json!(fresh_tween_id));
            }

            map_set(&mut tween_map, fresh_tween_id.clone(), duplicated);
            fresh_tween_links.push((input_id.clone(), fresh_tween_id));
        }

        for input_id in &copied_input_ids {
            let Some(link) = input_links.get(input_id) else {
                continue;
            };

            let is_tween = source_uses_tween_descriptor(link.get("source").and_then(Value::as_str));
            let fresh_tween_id = fresh_tween_links
                .iter()
                .find(|(id, _)| id == input_id)
                .map(|(_, fresh)| fresh.clone());

            // A tween link without a duplicated tween would be dangling.
            if is_tween && fresh_tween_id.is_none() {
                continue;
            }

            let mut copied_link = link.clone();

            if let Some(object) = copied_link.as_object_mut() {
                if let Some(fresh_tween_id) = &fresh_tween_id {
                    object.insert("id".to_string(), json!(fresh_tween_id));
                } else if let Some(link_id) = object.get("id").and_then(truthy_key)
                    && let Some(fresh) = lookup_fresh(&input_id_map, &link_id)
                {
                    object.insert("id".to_string(), json!(fresh));
                }
            }

            let fresh_link_input_id = remap_id(&input_id_map, input_id);
            input_links.insert(fresh_link_input_id, copied_link);
        }
    }

    let tweens = match tween_container {
        TweenContainer::Array => Some(Value::Array(
            tween_map.into_iter().map(|(_, tween)| tween).collect(),
        )),
        TweenContainer::Object => Some(Value::Object(tween_map.into_iter().collect())),
        TweenContainer::Omitted => None,
    };

    // Expression assignments are kept keyed by assignment id, and only for the
    // inputs the copy carries.
    let to_expressions = object_or_empty(to_scene.get("expressions"));
    let mut assignments = to_expressions
        .get("assignments")
        .and_then(Value::as_object)
        .map(clone_value_map)
        .unwrap_or_default();

    if let Some(source_assignments) = source_scene
        .get("expressions")
        .and_then(|expressions| expressions.get("assignments"))
        .and_then(Value::as_object)
    {
        for assignment in source_assignments
            .values()
            .filter(|assignment| is_truthy(assignment))
        {
            let Some(input_id) = assignment.get("inputId").and_then(Value::as_str) else {
                continue;
            };

            if !copied_input_ids.iter().any(|id| id == input_id) {
                continue;
            }

            let Some(assignment_id) = assignment.get("id").and_then(truthy_key) else {
                continue;
            };

            assignments.insert(assignment_id, assignment.clone());

            // Same-Scene copies duplicate the assignment onto fresh ids, with
            // `inputId` pointing at the copied input.
            if remap {
                let fresh_assignment_id = fresh_copy_id("assignment");
                let mut duplicated = assignment.clone();

                if let Some(object) = duplicated.as_object_mut() {
                    object.insert("id".to_string(), json!(fresh_assignment_id));
                    object.insert(
                        "inputId".to_string(),
                        json!(remap_id(&input_id_map, input_id)),
                    );
                }

                assignments.insert(fresh_assignment_id, duplicated);
            }
        }
    }

    let mut data = Map::new();
    data.insert("modules".to_string(), Value::Object(modules));
    data.insert("groups".to_string(), Value::Array(groups));
    data.insert(
        "inputs".to_string(),
        json!({
            "focusedInput": focused_input,
            "inputs": Value::Object(inputs),
            "inputLinks": Value::Object(input_links),
        }),
    );

    if let Some(tweens) = tweens {
        data.insert("tweens".to_string(), tweens);
    }

    data.insert(
        "expressions".to_string(),
        json!({"assignments": Value::Object(assignments)}),
    );

    Some(Value::Object(data))
}

#[cfg(test)]
mod tests {
    //! Port of `tests/unit/scene-cell.test.js` (Vitest).
    use super::*;

    fn group(id: &str, name: &str, modules: Value) -> Value {
        json!({"id": id, "name": name, "modules": modules})
    }

    fn scene(extra: Value) -> Value {
        let mut base = json!({
            "id": "scene-1",
            "name": "Scene 1",
            "groups": [
                group("g0", "Background", json!(["m0"])),
                group("g1", "Foreground", json!(["m1", "m2"])),
            ],
            "modules": {
                "m0": {"id": "m0"},
                "m1": {"id": "m1"},
                "m2": {"id": "m2"},
            },
            "inputs": {
                "focusedInput": {"id": null, "title": null},
                "inputs": {"i0": {"id": "i0"}},
                "inputLinks": {"i0": {"source": "bass"}},
            },
            "expressions": {"assignments": {"a0": {"inputId": "i0"}}},
            "tweens": [{"id": "t0"}],
        });

        if let (Some(target), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }

        base
    }

    fn cell() -> Value {
        json!({
            "group": group("live", "Foreground", json!(["m3"])),
            "modules": {"m3": {"id": "m3"}},
            "inputs": {
                "inputs": {"i3": {"id": "i3"}},
                "inputLinks": {"i3": {"source": "treble"}},
            },
            "expressions": {"assignments": {"a3": {"inputId": "i3"}}},
            "tweens": [{"id": "t3"}],
        })
    }

    // ————————————————— clearSceneCellModules —————————————————

    #[test]
    fn clear_removes_the_cells_modules_and_their_snapshots() {
        // scene-cell.test.js:36-43
        let result = clear_scene_cell_modules(&scene(json!({})), 1, &[]).unwrap();

        assert_eq!(result.module_ids, vec!["m1", "m2"]);
        assert_eq!(result.groups[1]["modules"], json!([]));
        assert_eq!(result.groups[0]["modules"], json!(["m0"]));
        assert_eq!(
            result.modules.keys().cloned().collect::<Vec<_>>(),
            vec!["m0"]
        );
    }

    #[test]
    fn clear_returns_none_for_an_empty_cell() {
        // scene-cell.test.js:45-54
        assert!(clear_scene_cell_modules(&scene(json!({})), 5, &[]).is_none());
        assert!(
            clear_scene_cell_modules(
                &scene(json!({"groups": [null, group("g1", "F", json!([]))]})),
                0,
                &[]
            )
            .is_none()
        );
    }

    #[test]
    fn clear_uses_named_rows_to_find_the_cells_group() {
        // scene-cell.test.js:56-70
        let value = scene(json!({
            "groups": [
                group("ga", "Effects", json!(["m0"])),
                group("gb", "Background", json!(["m1"])),
            ],
            "modules": {"m0": {"id": "m0"}, "m1": {"id": "m1"}},
        }));
        let row_names = vec!["".to_string(), "".to_string(), "Background".to_string()];

        let result = clear_scene_cell_modules(&value, 2, &row_names).unwrap();

        assert_eq!(result.groups[1]["modules"], json!([]));
        assert_eq!(result.groups[0]["modules"], json!(["m0"]));
        assert_eq!(Value::Object(result.modules), json!({"m0": {"id": "m0"}}));
    }

    #[test]
    fn clear_does_not_mutate_the_scene() {
        // scene-cell.test.js:72-78
        let value = scene(json!({}));
        let result = clear_scene_cell_modules(&value, 1, &[]).unwrap();

        assert_eq!(value["groups"][1]["modules"], json!(["m1", "m2"]));
        assert_ne!(Value::Array(result.groups), value["groups"]);
    }

    // ————————————————— mergeSceneCell —————————————————

    #[test]
    fn merge_replaces_the_group_at_the_cells_row() {
        // scene-cell.test.js:93-99
        let result = merge_scene_cell(&scene(json!({})), 1, &[], &cell()).unwrap();

        assert_eq!(result["groups"][1]["id"], json!("live"));
        assert_eq!(result["groups"][0]["id"], json!("g0"));
        assert_eq!(result["groups"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn merge_appends_a_group_when_the_row_has_no_snapshot() {
        // scene-cell.test.js:101-106
        let result = merge_scene_cell(&scene(json!({"groups": [null]})), 1, &[], &cell()).unwrap();

        assert!(result["groups"][0].is_null());
        assert_eq!(result["groups"][1]["id"], json!("live"));
    }

    #[test]
    fn merge_pads_positional_rows_below_the_saved_groups() {
        // scene-cell.test.js:108-116
        let result = merge_scene_cell(&scene(json!({})), 3, &[], &cell()).unwrap();

        assert_eq!(result["groups"].as_array().unwrap().len(), 4);
        assert_eq!(result["groups"][0]["id"], json!("g0"));
        assert_eq!(result["groups"][1]["id"], json!("g1"));
        assert!(result["groups"][2].is_null());
        assert_eq!(result["groups"][3]["id"], json!("live"));
    }

    #[test]
    fn merge_appends_a_named_row_cell_without_padding() {
        // scene-cell.test.js:118-126
        let mut named_cell = cell();
        named_cell["group"] = group("live", "Webcam", json!(["m3"]));
        let row_names = vec![
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "Webcam".to_string(),
        ];

        let result = merge_scene_cell(&scene(json!({})), 5, &row_names, &named_cell).unwrap();

        assert_eq!(result["groups"][2]["id"], json!("live"));
        assert_eq!(result["groups"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn merge_merges_module_snapshots_and_prunes_unreferenced_ones() {
        // scene-cell.test.js:128-135
        let result = merge_scene_cell(&scene(json!({})), 1, &[], &cell()).unwrap();
        let modules = result["modules"].as_object().unwrap();

        assert!(modules.contains_key("m0"));
        assert!(modules.contains_key("m3"));
        assert!(!modules.contains_key("m1"));
        assert!(!modules.contains_key("m2"));
    }

    #[test]
    fn merge_merges_inputs_links_expressions_and_tweens() {
        // scene-cell.test.js:137-146
        let result = merge_scene_cell(&scene(json!({})), 1, &[], &cell()).unwrap();

        assert!(result["inputs"]["inputs"].get("i0").is_some());
        assert!(result["inputs"]["inputs"].get("i3").is_some());
        assert!(result["inputs"]["inputLinks"].get("i3").is_some());
        assert!(result["expressions"]["assignments"].get("a0").is_some());
        assert!(result["expressions"]["assignments"].get("a3").is_some());
        assert_eq!(result["tweens"], json!([{"id": "t0"}, {"id": "t3"}]));
    }

    #[test]
    fn merge_uses_named_rows_to_replace_the_right_group() {
        // scene-cell.test.js:148-161
        let value = scene(json!({
            "groups": [
                group("ga", "Effects", json!(["m0"])),
                group("gb", "Background", json!(["m1"])),
            ],
            "modules": {"m0": {"id": "m0"}, "m1": {"id": "m1"}},
        }));
        let row_names = vec!["".to_string(), "".to_string(), "Background".to_string()];

        let result = merge_scene_cell(&value, 2, &row_names, &cell()).unwrap();

        assert_eq!(result["groups"][1]["id"], json!("live"));
        assert_eq!(result["groups"][0]["id"], json!("ga"));
    }

    #[test]
    fn merge_does_not_mutate_the_scene() {
        // scene-cell.test.js:163-169
        let value = scene(json!({}));
        let result = merge_scene_cell(&value, 1, &[], &cell()).unwrap();

        assert_eq!(value["groups"][1]["id"], json!("g1"));
        assert_ne!(result["groups"], value["groups"]);
    }

    #[test]
    fn merge_returns_none_without_a_cell_group() {
        assert!(merge_scene_cell(&scene(json!({})), 0, &[], &json!({})).is_none());
        assert!(merge_scene_cell(&scene(json!({})), 0, &[], &json!({"group": null})).is_none());
    }

    #[test]
    fn merge_keeps_the_tween_container_shape() {
        // scene-cell.js:37-69
        let value = scene(json!({"tweens": {"t0": {"id": "t0"}}}));
        let result = merge_scene_cell(&value, 1, &[], &cell()).unwrap();

        assert_eq!(
            result["tweens"],
            json!({"t0": {"id": "t0"}, "t3": {"id": "t3"}})
        );
    }

    // ————————————————— copySceneCellToScene —————————————————

    fn copy(source: &Value, target: &Value, from_row: usize, to_row: usize) -> Option<Value> {
        copy_scene_cell(source, target, from_row, to_row, &[])
    }

    fn same_scene_copy_fixture() -> Value {
        json!({
            "id": "scene-a",
            "groups": [
                {
                    "id": "g-source",
                    "name": "Background",
                    "enabledInputId": "i-enabled",
                    "modules": ["m-source"],
                },
                {
                    "id": "g-target",
                    "name": "Effects",
                    "modules": ["m-target"],
                },
            ],
            "modules": {
                "m-source": {
                    "id": "m-source",
                    "meta": {"enabledInputId": "i-enabled"},
                    "$props": {"amount": {"id": "i-amount", "type": "float"}},
                },
                "m-target": {
                    "id": "m-target",
                    "$props": {"colour": {"id": "i-colour", "type": "color"}},
                },
            },
            "inputs": {
                "focusedInput": {"id": "target-focus"},
                "inputs": {
                    "i-enabled": {"value": true},
                    "i-amount": {"value": 0.25},
                    "i-colour": {"value": "red"},
                },
                "inputLinks": {
                    "i-amount": {"source": "tween", "id": "tw-amount"},
                    "i-colour": {"source": "bass"},
                },
            },
            "expressions": {
                "assignments": {
                    "a-source": {"id": "a-source", "inputId": "i-amount"},
                    "a-target": {"id": "a-target", "inputId": "i-colour"},
                },
            },
            "tweens": [
                {"id": "tw-amount", "duration": 2},
                {"id": "tw-target", "duration": 4},
            ],
        })
    }

    #[test]
    fn copy_allows_a_different_row_of_the_same_scene() {
        let scene = same_scene_copy_fixture();
        let original = scene.clone();
        let source_group = scene["groups"][0].clone();
        let target_group = scene["groups"][1].clone();
        assert_ne!(source_group, target_group, "the fixture has distinct rows");

        let data = copy_scene_cell(&scene, &scene, 0, 1, &[]).unwrap();

        assert_ne!(
            data["groups"][1]["id"], "g-source",
            "a same-Scene copy must mint a fresh Group id"
        );
        assert_eq!(
            data["groups"][0], source_group,
            "the source row is untouched"
        );

        let copied_group = &data["groups"][1];
        let copied_enabled_input_id = copied_group["enabledInputId"].as_str().unwrap();
        assert_ne!(
            copied_enabled_input_id, "i-enabled",
            "the copied Group must use a fresh input id"
        );

        let copied_modules = copied_group["modules"].as_array().unwrap();
        assert_eq!(copied_modules.len(), 1);
        let copied_module_id = copied_modules[0].as_str().unwrap();
        assert_ne!(
            copied_module_id, "m-source",
            "the copied Group must reference a fresh Module snapshot"
        );

        let modules = data["modules"].as_object().unwrap();
        assert!(modules.contains_key("m-source"));
        assert!(modules.contains_key("m-target"));
        assert!(
            modules.contains_key(copied_module_id),
            "the copied Module snapshot must be keyed by its fresh id"
        );
        let copied_module = &modules[copied_module_id];
        assert_eq!(copied_module["id"], json!(copied_module_id));
        assert_eq!(
            copied_module["meta"]["enabledInputId"],
            json!(copied_enabled_input_id),
            "Module meta input references must be remapped"
        );
        let copied_amount_input_id = copied_module["$props"]["amount"]["id"].as_str().unwrap();
        assert_ne!(
            copied_amount_input_id, "i-amount",
            "the copied prop must use a fresh input id"
        );

        let inputs = data["inputs"]["inputs"].as_object().unwrap();
        for original_id in ["i-enabled", "i-amount", "i-colour"] {
            assert!(
                inputs.contains_key(original_id),
                "the original input {original_id} must survive"
            );
        }
        assert_eq!(inputs["i-enabled"], json!({"value": true}));
        assert_eq!(inputs["i-amount"], json!({"value": 0.25}));
        assert_eq!(inputs["i-colour"], json!({"value": "red"}));
        assert_eq!(
            inputs[copied_enabled_input_id],
            json!({"value": true}),
            "the copied enabled input keeps its value"
        );
        assert_eq!(
            inputs[copied_amount_input_id],
            json!({"value": 0.25}),
            "the copied amount input keeps its value"
        );

        let links = data["inputs"]["inputLinks"].as_object().unwrap();
        assert_eq!(
            links["i-amount"],
            json!({"source": "tween", "id": "tw-amount"}),
            "the original tween link must survive"
        );
        let copied_link = &links[copied_amount_input_id];
        assert_eq!(copied_link["source"], json!("tween"));
        let copied_tween_id = copied_link["id"].as_str().unwrap();
        assert_ne!(
            copied_tween_id, "tw-amount",
            "the copied tween link must use a fresh tween id"
        );
        assert_eq!(copied_link["id"], json!(copied_tween_id));

        let tweens = data["tweens"].as_array().unwrap();
        let original_tween = tweens
            .iter()
            .find(|tween| tween["id"] == "tw-amount")
            .expect("the original tween must survive");
        assert_eq!(original_tween, &json!({"id": "tw-amount", "duration": 2}));
        assert!(tweens.iter().any(|tween| tween["id"] == "tw-target"));
        let copied_tween = tweens
            .iter()
            .find(|tween| tween["id"] == copied_tween_id)
            .expect("the copied tween must be present");
        assert_eq!(copied_tween["id"], json!(copied_tween_id));
        assert_eq!(copied_tween["duration"], json!(2));

        let assignments = data["expressions"]["assignments"].as_object().unwrap();
        assert_eq!(
            assignments["a-source"],
            json!({"id": "a-source", "inputId": "i-amount"})
        );
        assert_eq!(
            assignments["a-target"],
            json!({"id": "a-target", "inputId": "i-colour"})
        );
        let copied_assignments = assignments
            .iter()
            .filter(|(id, _)| id.as_str() != "a-source" && id.as_str() != "a-target")
            .collect::<Vec<_>>();
        assert_eq!(copied_assignments.len(), 1);
        let (copied_assignment_id, copied_assignment) = copied_assignments[0];
        assert_ne!(*copied_assignment_id, "a-source");
        assert_ne!(*copied_assignment_id, "a-target");
        assert_eq!(copied_assignment["id"], json!(copied_assignment_id));
        assert_eq!(
            copied_assignment["inputId"],
            json!(copied_amount_input_id),
            "the copied assignment must target the fresh input"
        );

        assert_eq!(data["inputs"]["focusedInput"]["id"], json!("target-focus"));
        assert_eq!(scene, original, "copying must not mutate the source Scene");

        assert!(
            copy_scene_cell(&scene, &scene, 0, 0, &[]).is_none(),
            "copying a row onto itself remains a no-op"
        );
    }

    #[test]
    fn copy_same_scene_remaps_the_canonical_module_id() {
        let scene = json!({
            "id": "scene-a",
            "groups": [
                {"id": "g-source", "modules": ["m-source"]},
                {"id": "g-target", "modules": []},
            ],
            "modules": {
                "m-source": {"id": "m-source", "$id": "m-source"},
            },
        });

        let data = copy_scene_cell(&scene, &scene, 0, 1, &[]).unwrap();
        let copied_module_id = data["groups"][1]["modules"][0]
            .as_str()
            .expect("copied Module id");
        assert_ne!(
            copied_module_id, "m-source",
            "the copied Group must reference a fresh Module snapshot"
        );

        let copied_module = &data["modules"][copied_module_id];
        assert_eq!(
            copied_module["$id"], copied_module_id,
            "the copied snapshot's canonical id must match its fresh map key"
        );
        assert_eq!(
            copied_module["id"], copied_module_id,
            "the copied snapshot's id must match its fresh map key"
        );
    }

    #[test]
    fn copy_same_scene_clear_one_row_keeps_the_other_snapshot() {
        let scene = same_scene_copy_fixture();
        let original = scene.clone();
        let data = copy_scene_cell(&scene, &scene, 0, 1, &[]).unwrap();

        let source_module_id = scene["groups"][0]["modules"][0].as_str().unwrap();
        let copied_module_id = data["groups"][1]["modules"][0].as_str().unwrap();
        assert_ne!(
            source_module_id, copied_module_id,
            "same-Scene rows must not share a Module id"
        );

        let mut applied = scene.clone();
        applied["groups"] = data["groups"].clone();
        applied["modules"] = data["modules"].clone();
        let before_clear = applied.clone();

        let cleared = clear_scene_cell_modules(&applied, 0, &[]).unwrap();

        assert_eq!(cleared.module_ids, vec![source_module_id]);
        assert_eq!(cleared.groups[0]["modules"], json!([]));
        assert_eq!(cleared.groups[1]["modules"], json!([copied_module_id]));
        assert!(!cleared.modules.contains_key(source_module_id));
        assert!(
            cleared.modules.contains_key(copied_module_id),
            "clearing the source row must preserve the copied snapshot"
        );
        assert_eq!(
            cleared.modules[copied_module_id]["id"],
            json!(copied_module_id)
        );
        assert_eq!(applied, before_clear, "clearing must not mutate the Scene");
        assert_eq!(scene, original, "copying must not mutate the source Scene");
    }

    #[test]
    fn copy_carries_the_group_snapshots_inputs_and_links() {
        // scenes.js:3568-3735
        let from = json!({
            "id": "from",
            "name": "From",
            "groups": [{
                "id": "g0",
                "name": "Background",
                "alphaInputId": "g-alpha",
                "compositeOperationInputId": "g-composite",
                "modules": ["m1", "m2", "missing"],
            }],
            "modules": {
                "m1": {
                    "id": "m1",
                    "meta": {"alphaInputId": "m1-alpha"},
                    "$props": {
                        "colour": {"id": "m1-colour", "type": "color"},
                        "amount": {"id": "m1-amount", "type": "float"},
                    },
                },
                "m2": {
                    "id": "m2",
                    "meta": {"enabledInputId": "m2-enabled"},
                    "$props": {"position": {"id": "m2-position", "type": "vec2"}},
                },
            },
            "inputs": {
                "focusedInput": {"id": "from-focus", "title": "From"},
                "inputs": {
                    "g-alpha": {"value": 1},
                    "g-composite": {"value": "normal"},
                    "m1-alpha": {"value": 1},
                    "m1-colour": {"value": "red"},
                    "m1-colour-r": {"value": 1},
                    "m1-colour-g": {"value": 0},
                    "m1-colour-b": {"value": 0},
                    "m1-colour-a": {"value": 1},
                    "m1-amount": {"value": 0.5},
                    "m2-enabled": {"value": true},
                    "m2-position": {"value": [0, 0]},
                    "m2-position-0": {"value": 0},
                    "m2-position-1": {"value": 0},
                    "unrelated": {"value": 9},
                },
                "inputLinks": {
                    "m1-amount": {"source": "bass"},
                    "unrelated": {"source": "treble"},
                },
            },
            "expressions": {
                "assignments": {
                    "sa": {"id": "sa", "inputId": "m1-amount"},
                    "sb": {"id": "sb", "inputId": "unrelated"},
                },
            },
            "tweens": [],
        });

        let to = json!({
            "id": "to",
            "name": "To",
            "groups": [{"id": "tg0", "name": "Background", "modules": ["tm0"]}],
            "modules": {"tm0": {"id": "tm0"}},
            "inputs": {
                "focusedInput": {"id": "to-focus", "title": "To"},
                "inputs": {"tm0-value": {"value": 3}},
                "inputLinks": {"tm0-value": {"source": "mid"}},
            },
            "expressions": {"assignments": {"ta": {"id": "ta", "inputId": "tm0-value"}}},
            "tweens": [],
        });

        let result = copy(&from, &to, 0, 0).unwrap();

        // The Group is copied verbatim, including its module id list.
        assert_eq!(result["groups"][0], from["groups"][0]);
        assert_eq!(result["groups"].as_array().unwrap().len(), 1);

        // Both snapshots ride along; the target's own Module stays, and an id
        // with no snapshot adds nothing.
        assert_eq!(result["modules"]["m1"], from["modules"]["m1"]);
        assert_eq!(result["modules"]["m2"], from["modules"]["m2"]);
        assert!(result["modules"].get("tm0").is_some());
        assert!(result["modules"].get("missing").is_none());

        // The source's whole `inputs` map comes over (the JS spreads
        // `sourceInputs.inputs`), while links/expressions only follow the
        // copied ids.
        let inputs = result["inputs"]["inputs"].as_object().unwrap();

        for id in [
            "g-alpha",
            "g-composite",
            "m1-alpha",
            "m1-colour",
            "m1-colour-r",
            "m1-colour-g",
            "m1-colour-b",
            "m1-colour-a",
            "m1-amount",
            "m2-enabled",
            "m2-position",
            "m2-position-0",
            "m2-position-1",
            "tm0-value",
        ] {
            assert!(inputs.contains_key(id), "missing input {id}");
        }

        assert_eq!(inputs["unrelated"], json!({"value": 9}));

        let links = result["inputs"]["inputLinks"].as_object().unwrap();
        assert_eq!(links["m1-amount"], json!({"source": "bass"}));
        assert_eq!(links["tm0-value"], json!({"source": "mid"}));
        assert!(!links.contains_key("unrelated"));

        // The target keeps its focus and its expressions; only assignments for
        // copied inputs come over from the source.
        assert_eq!(result["inputs"]["focusedInput"]["id"], json!("to-focus"));

        let assignments = result["expressions"]["assignments"].as_object().unwrap();
        assert_eq!(
            assignments["sa"],
            json!({"id": "sa", "inputId": "m1-amount"})
        );
        assert!(assignments.contains_key("ta"));
        assert!(!assignments.contains_key("sb"));

        // A target without tween links keeps its (empty) container.
        assert_eq!(result["tweens"], json!([]));
    }

    #[test]
    fn copy_pads_missing_rows_with_holes_and_trims_trailing_holes() {
        // scenes.js:3631-3647
        let from = json!({
            "id": "from",
            "groups": [{"id": "g0", "name": "Background", "modules": ["m1"]}],
            "modules": {"m1": {"id": "m1"}},
        });
        let to = json!({
            "id": "to",
            "groups": [{"id": "tg0", "name": "Effects", "modules": ["tm0"]}],
            "modules": {"tm0": {"id": "tm0"}},
        });

        let result = copy(&from, &to, 0, 2).unwrap();

        assert_eq!(result["groups"].as_array().unwrap().len(), 3);
        assert_eq!(result["groups"][0]["id"], json!("tg0"));
        assert!(result["groups"][1].is_null());
        assert_eq!(result["groups"][2]["id"], json!("g0"));

        // Trailing holes in the target carry no meaning, so they are trimmed.
        let sparse_to = json!({
            "id": "to",
            "groups": [
                {"id": "tg0", "name": "Effects", "modules": ["tm0"]},
                Value::Null,
                Value::Null,
            ],
            "modules": {"tm0": {"id": "tm0"}},
        });

        let result = copy(&from, &sparse_to, 0, 0).unwrap();

        assert_eq!(result["groups"].as_array().unwrap().len(), 1);
        assert_eq!(result["groups"][0]["id"], json!("g0"));
    }

    #[test]
    fn copy_uses_named_rows_for_the_source_but_writes_positionally() {
        // scenes.js:3600-3610 and 3636-3643
        let from = json!({
            "id": "from",
            "groups": [
                {"id": "fg", "name": "Foreground", "modules": ["m-fg"]},
                {"id": "fx", "name": "Effects", "modules": ["m-fx"]},
            ],
            "modules": {"m-fg": {"id": "m-fg"}, "m-fx": {"id": "m-fx"}},
        });
        let to = json!({
            "id": "to",
            "groups": [
                {"id": "tfx", "name": "Effects", "modules": ["tm0"]},
                {"id": "tfg", "name": "Foreground", "modules": ["tm1"]},
            ],
            "modules": {"tm0": {"id": "tm0"}, "tm1": {"id": "tm1"}},
        });
        let row_names = vec!["Effects".to_string(), "Foreground".to_string()];

        // Row 1 is named "Foreground", so the source Group comes from index 0:
        // named lookup, not position. The write is `groups[toRowIndex]`, so it
        // lands on row 0 even though its name is "Foreground" and the target
        // has its own Foreground Group on row 1.
        let result = copy_scene_cell(&from, &to, 1, 0, &row_names).unwrap();

        assert_eq!(result["groups"][0]["id"], json!("fg"));
        assert_eq!(result["groups"][1]["id"], json!("tfg"));
        assert_eq!(result["groups"].as_array().unwrap().len(), 2);
        assert!(result["modules"].get("m-fg").is_some());
    }

    #[test]
    fn copy_returns_none_for_the_js_null_cases() {
        // scenes.js:3577-3599
        let from = json!({
            "id": "from",
            "groups": [{"id": "g0", "name": "A", "modules": ["m1"]}],
            "modules": {"m1": {"id": "m1"}},
        });
        let to = json!({"id": "to", "groups": [], "modules": {}});

        // No Group on the source row.
        assert!(copy(&from, &to, 3, 0).is_none());

        // The Group has no Modules.
        let empty = json!({
            "id": "from",
            "groups": [{"id": "g0", "name": "A", "modules": []}],
            "modules": {},
        });
        assert!(copy(&empty, &to, 0, 0).is_none());

        // The same Scene on both sides.
        assert!(copy(&from, &from, 0, 0).is_none());

        // A document with neither `groups` nor `layers` cannot be normalised.
        assert!(copy(&json!({"id": "from"}), &to, 0, 0).is_none());

        // The target must be a real Scene too.
        assert!(copy(&from, &json!({"groups": []}), 0, 0).is_none());
    }

    #[test]
    fn copy_carries_tween_links_and_drops_dangling_ones() {
        // scenes.js:3669-3705
        let from = json!({
            "id": "from",
            "groups": [{"id": "g0", "name": "Effects", "modules": ["m1"]}],
            "modules": {
                "m1": {
                    "id": "m1",
                    "$props": {
                        "amount": {"id": "i1", "type": "float"},
                        "rotation": {"id": "i2", "type": "float"},
                    },
                },
            },
            "inputs": {
                "inputs": {"i1": {"value": 0.5}, "i2": {"value": 0.25}},
                "inputLinks": {
                    "i1": {"source": "tween", "id": "tw-1"},
                    "i2": {"source": "tween", "id": "missing"},
                },
            },
            "tweens": [{"id": "tw-1", "duration": 4}],
        });
        let to = json!({
            "id": "to",
            "groups": [],
            "modules": {},
            "inputs": {"inputs": {}, "inputLinks": {}},
            "tweens": [{"id": "t0"}],
        });

        let result = copy(&from, &to, 0, 0).unwrap();

        assert_eq!(
            result["tweens"],
            json!([{"id": "t0"}, {"id": "tw-1", "duration": 4}])
        );

        let links = result["inputs"]["inputLinks"].as_object().unwrap();
        assert_eq!(links["i1"], json!({"source": "tween", "id": "tw-1"}));
        assert!(!links.contains_key("i2"));

        // Object-shaped target containers stay objects.
        let object_to = json!({
            "id": "to",
            "groups": [],
            "modules": {},
            "inputs": {"inputs": {}, "inputLinks": {}},
            "tweens": {"t0": {"id": "t0"}},
        });

        let result = copy(&from, &object_to, 0, 0).unwrap();

        assert_eq!(
            result["tweens"],
            json!({"t0": {"id": "t0"}, "tw-1": {"id": "tw-1", "duration": 4}})
        );
    }

    #[test]
    fn copy_carries_drawn_curves_like_tween_links() {
        let from = json!({
            "id": "from",
            "groups": [{"id": "g0", "name": "Effects", "modules": ["m1"]}],
            "modules": {
                "m1": {
                    "id": "m1",
                    "$props": {"amount": {"id": "i1", "type": "float"}},
                },
            },
            "inputs": {
                "inputs": {"i1": {"value": 0.5}},
                "inputLinks": {
                    "i1": {"source": "draw", "id": "draw-1"},
                },
            },
            "tweens": [{"id": "draw-1", "data": [[0], [1]], "easing": "linear"}],
        });
        let to = json!({
            "id": "to",
            "groups": [],
            "modules": {},
            "inputs": {"inputs": {}, "inputLinks": {}},
            "tweens": [],
        });

        let result = copy(&from, &to, 0, 0).unwrap();
        assert_eq!(
            result["tweens"],
            json!([{"id": "draw-1", "data": [[0], [1]], "easing": "linear"}])
        );
        assert_eq!(
            result["inputs"]["inputLinks"]["i1"],
            json!({"source": "draw", "id": "draw-1"})
        );
    }
}
