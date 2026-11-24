//! Store-level undo/redo of the preset graph.
//!
//! Undo/redo snapshots the preset graph (Groups, Modules, Inputs, expressions
//! and tweens). Every mutating user action records the prior state; undo/redo
//! restore those snapshots. A new edit after undo discards the redo branch, and
//! undo/redo on empty history are harmless no-ops.

use modv_app::store_ops::Store;
use serde_json::json;

fn create_group(store: &mut Store, id: &str, name: &str) {
    store
        .dispatch("groups/createGroup", json!({ "id": id, "name": name }))
        .expect("groups/createGroup dispatches");
}

fn group_name<'a>(store: &'a Store, id: &str) -> Option<&'a str> {
    store
        .state()
        .groups
        .iter()
        .find(|group| group.id == id)
        .map(|group| group.name.as_str())
}

fn rename_group(store: &mut Store, id: &str, name: &str) {
    store
        .dispatch(
            "groups/updateGroupName",
            json!({ "groupId": id, "name": name }),
        )
        .expect("groups/updateGroupName dispatches");
}

#[test]
fn undo_restores_a_group_rename_and_redo_reapplies_it() {
    let mut store = Store::new();
    create_group(&mut store, "g1", "Before");
    rename_group(&mut store, "g1", "After");
    assert_eq!(group_name(&store, "g1"), Some("After"));

    store
        .dispatch("history/undo", json!({}))
        .expect("history/undo dispatches");
    assert_eq!(
        group_name(&store, "g1"),
        Some("Before"),
        "undo must restore the state recorded before the rename"
    );

    store
        .dispatch("history/redo", json!({}))
        .expect("history/redo dispatches");
    assert_eq!(
        group_name(&store, "g1"),
        Some("After"),
        "redo must reapply the undone rename"
    );
}

#[test]
fn undo_of_create_removes_the_group_and_redo_brings_it_back() {
    let mut store = Store::new();
    create_group(&mut store, "g1", "Created");

    store
        .dispatch("history/undo", json!({}))
        .expect("history/undo dispatches");
    assert_eq!(
        store.state().groups.len(),
        0,
        "undo of a create must remove the Group"
    );

    store
        .dispatch("history/redo", json!({}))
        .expect("history/redo dispatches");
    assert_eq!(store.state().groups.len(), 1, "redo must restore the Group");
    assert_eq!(
        group_name(&store, "g1"),
        Some("Created"),
        "the restored Group keeps its id and name"
    );
}

#[test]
fn a_new_edit_after_undo_clears_redo() {
    let mut store = Store::new();
    create_group(&mut store, "g1", "Before");
    rename_group(&mut store, "g1", "After");

    store
        .dispatch("history/undo", json!({}))
        .expect("history/undo dispatches");
    assert_eq!(
        group_name(&store, "g1"),
        Some("Before"),
        "the branch test needs a real undo first"
    );

    rename_group(&mut store, "g1", "Branch");
    store
        .dispatch("history/redo", json!({}))
        .expect("history/redo dispatches");
    assert_eq!(
        group_name(&store, "g1"),
        Some("Branch"),
        "a new edit after undo discards the redo branch"
    );
}

#[test]
fn undo_and_redo_on_empty_history_are_no_ops() {
    let mut store = Store::new();

    let undo = store.dispatch("history/undo", json!({}));
    let redo = store.dispatch("history/redo", json!({}));

    assert!(
        undo.is_ok(),
        "undo on an empty history must not error: {undo:?}"
    );
    assert!(
        redo.is_ok(),
        "redo on an empty history must not error: {redo:?}"
    );
    assert!(store.state().groups.is_empty());
}

#[test]
fn an_edit_that_does_not_change_the_graph_is_not_recorded() {
    let mut store = Store::new();
    create_group(&mut store, "g1", "Before");
    // Renaming to the current name succeeds but leaves the graph untouched.
    rename_group(&mut store, "g1", "Before");

    // The only recorded edit is the create, so one undo must remove the Group.
    // If the no-op rename had been recorded, this undo would leave it in place.
    store
        .dispatch("history/undo", json!({}))
        .expect("history/undo dispatches");
    assert!(
        store.state().groups.is_empty(),
        "a successful but state-preserving edit must not enter the history"
    );
}

#[test]
fn a_continuous_prop_drag_is_one_undo_step() {
    let mut store = Store::new();
    let module = store
        .dispatch(
            "modules/makeActiveModule",
            json!({ "moduleName": "Polygon" }),
        )
        .expect("makeActiveModule dispatches");
    let module_id = module["$id"].as_str().expect("module id").to_string();

    let set_prop = |store: &mut Store, value: f64| {
        store
            .dispatch(
                "modules/updateProp",
                json!({
                    "moduleId": module_id,
                    "prop": "strokeWeight",
                    "data": { "value": value },
                }),
            )
            .expect("modules/updateProp dispatches");
    };
    let prop = |store: &Store| {
        store.state().modules[&module_id].props["strokeWeight"]
            .as_f64()
            .expect("numeric prop")
    };

    // Establish a known starting value, then make the history start there.
    set_prop(&mut store, 1.0);
    store.clear_history();

    // A slider drag emits one update per frame. They are one user edit, so the
    // history must keep a single pre-drag snapshot rather than one per frame.
    for value in 2..=12 {
        set_prop(&mut store, f64::from(value));
    }
    assert_eq!(prop(&store), 12.0);

    store
        .dispatch("history/undo", json!({}))
        .expect("history/undo dispatches");
    assert_eq!(
        prop(&store),
        1.0,
        "one undo must return to the value before the whole drag"
    );

    store
        .dispatch("history/redo", json!({}))
        .expect("history/redo dispatches");
    assert_eq!(
        prop(&store),
        12.0,
        "redo must restore the value the drag ended on"
    );
}

#[test]
fn history_is_bounded_to_one_hundred_entries() {
    let mut store = Store::new();
    // 101 recorded creates; only the newest 100 prior-graph snapshots survive.
    for index in 0..101 {
        create_group(&mut store, &format!("g{index}"), "Group");
    }
    assert_eq!(store.state().groups.len(), 101);

    // Draining the stack with more undo calls than entries is harmless. The
    // oldest surviving snapshot still holds g0, which cannot be undone past.
    for _ in 0..101 {
        store
            .dispatch("history/undo", json!({}))
            .expect("history/undo dispatches");
    }
    assert_eq!(
        store.state().groups.len(),
        1,
        "the 100-entry bound drops the oldest snapshot"
    );
    assert_eq!(group_name(&store, "g0"), Some("Group"));
}
