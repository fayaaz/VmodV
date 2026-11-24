//! Startup splash/welcome surface.
//!
//! The splash reuses the existing About window slot (`UiState::show_about`) so
//! the ~80 kittest harnesses that construct `UiState::new()` are unaffected.
//! The app opens it once at startup by setting `show_about = true`; this test
//! drives the shell exactly as the app does and requires:
//!   * an accesskit label containing "VmodV splash",
//!   * the existing About build/profile information to remain visible,
//!   * a "Continue" button that clears `show_about` and removes the splash,
//!   * Escape still dismisses it,
//!   * a fresh state (show_about == false) shows no splash at all.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{EngineSnapshot, UiState};

fn harness(state: UiState) -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot::default();
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

#[test]
fn startup_splash_is_shown_in_the_about_slot_and_can_be_dismissed() {
    // This is what app startup does: reuse the About slot as the splash.
    let mut state = UiState::new();
    state.show_about = true;
    let mut harness = harness(state);
    harness.run();

    harness
        .query_all_by_label_contains("VmodV splash")
        .next()
        .expect("the startup splash must expose an accesskit label containing 'VmodV splash'");

    assert!(
        harness
            .query_all_by_label_contains("Adapter:")
            .next()
            .is_some(),
        "the splash reuses the About window, so its build/profile information (Adapter) must remain"
    );
    assert!(
        harness
            .query_all_by_label_contains("Backend:")
            .next()
            .is_some(),
        "the splash reuses the About window, so its build/profile information (Backend) must remain"
    );

    harness.get_by_label("Continue").click();
    harness.run();

    assert!(
        !harness.state().show_about,
        "clicking Continue must dismiss the splash"
    );
    assert!(
        harness
            .query_all_by_label_contains("VmodV splash")
            .next()
            .is_none(),
        "the 'VmodV splash' label must be gone once the splash is dismissed"
    );
}

#[test]
fn a_fresh_state_does_not_open_the_splash() {
    let mut harness = harness(UiState::new());
    harness.run();

    assert!(!harness.state().show_about);
    assert!(
        harness
            .query_all_by_label_contains("VmodV splash")
            .next()
            .is_none(),
        "the splash is startup-only: a default state must show no splash label"
    );
}

#[test]
fn escape_dismisses_the_startup_splash() {
    let mut state = UiState::new();
    state.show_about = true;
    let mut harness = harness(state);
    harness.run();

    harness
        .query_all_by_label_contains("VmodV splash")
        .next()
        .expect("the startup splash must be visible before pressing Escape");

    harness.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();

    assert!(
        !harness.state().show_about,
        "Escape must dismiss the splash"
    );
    assert!(
        harness
            .query_all_by_label_contains("VmodV splash")
            .next()
            .is_none(),
        "the 'VmodV splash' label must be gone after Escape"
    );
}
