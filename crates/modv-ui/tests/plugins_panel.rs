//! Kittest coverage for the opt-in JavaScript-plugin panel.
//!
//! The native Plugins tab replaces the old disabled placeholder: the master
//! switch is gated by a consent dialog (the first enable never runs anything
//! without confirmation), the discovered scripts are listed with their status,
//! and prop edits become [`UiCommand::SetPluginProp`] commands the shell
//! applies through `PluginHost::set_prop`.
//!
//! These are interaction tests; `plugins_snapshot.rs` is the golden image.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::state::SidePanelTab;
use modv_ui::{EngineSnapshot, PluginInfoView, PluginView, UiCommand, UiState};

fn plugin(name: &str, loaded: bool) -> PluginInfoView {
    PluginInfoView {
        path: format!("/home/user/plugins/{name}.js"),
        name: name.to_string(),
        author: "modV tests".to_string(),
        version: "1.2.3".to_string(),
        enabled: true,
        active: loaded,
        loaded,
        failed: false,
        error: None,
        source_bytes: 512,
        capture: None,
        props: Vec::new(),
    }
}

fn snapshot_with(plugins: PluginView) -> EngineSnapshot {
    EngineSnapshot {
        plugins,
        ..Default::default()
    }
}

fn build(snapshot: EngineSnapshot) -> Harness<'static, UiState> {
    let mut state = UiState::new();
    state.side_panel_tab = SidePanelTab::Plugins;
    Harness::builder()
        .with_size([1280.0, 900.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

fn taken_commands(harness: &mut Harness<'_, UiState>) -> Vec<UiCommand> {
    harness.state_mut().take_commands()
}

#[test]
fn the_first_enable_opens_the_consent_dialog_and_runs_nothing() {
    let mut harness = build(snapshot_with(PluginView {
        plugins: vec![plugin("Lumen", false)],
        ..Default::default()
    }));
    harness.run();

    // The panel states the risk before anything can be switched on.
    harness.get_by_label("Advanced: JavaScript plugins");
    harness.get_by_label(
        "Plugins are third-party JavaScript. In the native shell they run in a \
         restricted Boa sandbox, not a security boundary.",
    );
    harness.get_by_label("Not running");

    harness
        .get_by_label("Enable JavaScript plugins (advanced)")
        .click();
    harness.run();

    assert!(
        harness.state().show_plugin_consent,
        "the first enable opens the consent dialog"
    );
    assert!(
        taken_commands(&mut harness).is_empty(),
        "clicking the checkbox alone must not enable anything"
    );

    assert!(
        harness
            .get_all_by_label_contains("arbitrary JavaScript")
            .count()
            > 0
    );
    assert!(
        harness
            .get_all_by_label_contains("no filesystem or network globals")
            .count()
            > 0
    );
}

#[test]
fn confirming_the_dialog_grants_consent_and_enables_plugins() {
    let mut harness = build(snapshot_with(PluginView::default()));
    harness.run();

    harness
        .get_by_label("Enable JavaScript plugins (advanced)")
        .click();
    harness.run();
    harness.get_by_label("Enable plugins").click();
    harness.run();

    assert!(
        !harness.state().show_plugin_consent,
        "confirming closes the dialog"
    );
    assert_eq!(
        taken_commands(&mut harness),
        vec![
            UiCommand::SetPluginConsent(true),
            UiCommand::SetPluginsEnabled(true),
        ],
        "consent is recorded before the master switch turns on"
    );
}

#[test]
fn cancelling_the_dialog_leaves_plugins_off() {
    let mut harness = build(snapshot_with(PluginView::default()));
    harness.run();

    harness
        .get_by_label("Enable JavaScript plugins (advanced)")
        .click();
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(!harness.state().show_plugin_consent);
    assert!(taken_commands(&mut harness).is_empty());
}

#[test]
fn a_consented_enable_needs_no_second_dialog() {
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        ..Default::default()
    }));
    harness.run();

    harness
        .get_by_label("Enable JavaScript plugins (advanced)")
        .click();
    harness.run();

    assert!(!harness.state().show_plugin_consent);
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginsEnabled(true)]
    );
}

#[test]
fn disabling_is_immediate_and_revoking_clears_consent() {
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        loaded_count: 1,
        plugins: vec![plugin("Lumen", true)],
        ..Default::default()
    }));
    harness.run();
    harness.get_by_label("1 of 1 plugin loaded");

    harness
        .get_by_label("Enable JavaScript plugins (advanced)")
        .click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginsEnabled(false)]
    );

    harness.get_by_label("Revoke consent").click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginConsent(false)],
        "revoking is a separate action from the master switch"
    );
}

#[test]
fn a_gated_panel_explains_why_the_plugin_list_is_greyed() {
    let mut harness = build(snapshot_with(PluginView {
        plugins: vec![plugin("Lumen", false)],
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label(
        "JavaScript plugins need your consent. Review the warning above and enable \
         them; the scripts below are discovered but never evaluated.",
    );

    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        plugins: vec![plugin("Lumen", false)],
        ..Default::default()
    }));
    harness.run();
    harness.get_by_label(
        "JavaScript plugins are turned off. The scripts below are discovered but \
         not running; enable them to load their controls.",
    );
}
