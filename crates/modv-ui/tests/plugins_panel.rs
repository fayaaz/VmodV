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

#[test]
fn the_plugin_list_shows_status_metadata_path_and_capture() {
    let mut info = plugin("Lumen", true);
    info.capture = Some([160, 90]);
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        loaded_count: 1,
        plugins: vec![info],
        capture: Some([160, 90]),
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label("Lumen");
    harness.get_by_label("Loaded");
    harness.get_by_label("modV tests · v1.2.3");
    harness.get_by_label("/home/user/plugins/Lumen.js");
    harness.get_by_label("capture 160×90");
    harness.get_by_label("capture 160×90 at up to 20 Hz");
    harness.get_by_label("1 of 1 plugin loaded");
}

#[test]
fn toggling_a_plugin_rescans_and_reloads_through_commands() {
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        loaded_count: 1,
        plugins: vec![plugin("Lumen", true)],
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label("Enabled").click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginEnabled {
            path: "/home/user/plugins/Lumen.js".to_string(),
            enabled: false,
        }]
    );

    harness.get_by_label("Rescan").click();
    harness.run();
    assert_eq!(taken_commands(&mut harness), vec![UiCommand::RescanPlugins]);

    harness.get_by_label("Reload").click();
    harness.run();
    assert_eq!(taken_commands(&mut harness), vec![UiCommand::ReloadPlugins]);
}

fn prop(
    name: &str,
    kind: modv_ui::PluginPropKindView,
    value: serde_json::Value,
) -> modv_ui::PluginPropView {
    modv_ui::PluginPropView {
        name: name.to_string(),
        kind,
        value: value.clone(),
        default: value,
        min: None,
        max: None,
        step: None,
        options: Vec::new(),
    }
}

fn loaded_with_props() -> PluginInfoView {
    let mut info = plugin("Lumen", true);
    info.props = vec![
        modv_ui::PluginPropView {
            min: Some(0.0),
            max: Some(10.0),
            step: Some(1.0),
            ..prop(
                "count",
                modv_ui::PluginPropKindView::Int,
                serde_json::json!(4),
            )
        },
        prop(
            "toggle",
            modv_ui::PluginPropKindView::Bool,
            serde_json::json!(false),
        ),
        prop(
            "label",
            modv_ui::PluginPropKindView::Text,
            serde_json::json!("initial"),
        ),
        prop(
            "tint",
            modv_ui::PluginPropKindView::Color,
            serde_json::json!({ "r": 0.1, "g": 0.2, "b": 0.3, "a": 1.0 }),
        ),
        modv_ui::PluginPropView {
            options: vec![
                modv_ui::PluginPropOptionView {
                    label: "Wide".to_string(),
                    value: serde_json::json!("wide"),
                },
                modv_ui::PluginPropOptionView {
                    label: "Square".to_string(),
                    value: serde_json::json!("square"),
                },
            ],
            ..prop(
                "shape",
                modv_ui::PluginPropKindView::Select,
                serde_json::json!("wide"),
            )
        },
    ];
    info
}

#[test]
fn prop_controls_emit_the_exact_set_plugin_prop_command() {
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        loaded_count: 1,
        plugins: vec![loaded_with_props()],
        ..Default::default()
    }));
    harness.run();

    // Bool: click the generated checkbox.
    harness.get_by_label("toggle").click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginProp {
            path: "/home/user/plugins/Lumen.js".to_string(),
            prop: "toggle".to_string(),
            value: serde_json::json!(true),
        }]
    );

    // Select: the ComboBox exposes the current option as its value.
    harness.get_by_value("Wide").click();
    harness.run();
    harness.get_by_label("Square").click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::SetPluginProp {
            path: "/home/user/plugins/Lumen.js".to_string(),
            prop: "shape".to_string(),
            value: serde_json::json!("square"),
        }]
    );

    // Text: typing in the field queues the edited string.
    {
        let input = harness
            .get_all_by_role(accesskit::Role::TextInput)
            .find(|node| node.value().as_deref() == Some("initial"))
            .expect("the text prop's field");
        input.focus();
        input.type_text("edited");
    }
    harness.run();
    let commands = taken_commands(&mut harness);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            UiCommand::SetPluginProp { prop, value, .. }
                if prop == "label"
                    && value.as_str().is_some_and(|text| text.ends_with("edited"))
        )),
        "the text edit emits its current string: {commands:?}"
    );

    // The numeric and colour controls are present (their payload builders are
    // covered by the model unit tests).
    harness.get_by_label("count");
    harness.get_by_label("tint");
}

#[test]
fn plugin_errors_and_runtime_logs_are_rendered() {
    let mut info = plugin("Lumen", true);
    info.failed = false;
    info.error = Some("hook threw: boom".to_string());
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        loaded_count: 1,
        plugins: vec![info],
        logs: vec![
            "plugin: info Lumen: init".to_string(),
            "plugin: error Lumen: hook threw: boom".to_string(),
        ],
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label("Error");
    harness.get_by_label("hook threw: boom");
    // The log section opens by default when it holds an error.
    harness.get_by_label("plugin: error Lumen: hook threw: boom");
    harness.get_by_label("plugin: info Lumen: init");

    harness.get_by_label("Clear").click();
    harness.run();
    assert_eq!(
        taken_commands(&mut harness),
        vec![UiCommand::ClearPluginLogs]
    );
}

#[test]
fn an_empty_discovery_points_at_the_plugin_folders() {
    let mut harness = build(snapshot_with(PluginView {
        consented: true,
        enabled: true,
        ..Default::default()
    }));
    harness.run();

    harness.get_by_label_contains("No plugin scripts found");
    harness.get_by_label_contains("plugin/ folder");
}
