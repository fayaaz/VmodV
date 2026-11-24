//! Startup splash: the VmodV logo on a dim backdrop that fades out on its own.
//!
//! The splash is separate from the About dialog (`UiState::show_about`), which
//! keeps working as a normal window. A fresh `UiState` shows no splash, so the
//! many harnesses that construct one by default are unaffected.
//!
//! Time is driven by `Harness::with_step_dt`: egui advances `input.time` by the
//! step delta on every frame, so the fade is deterministic in tests.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use modv_ui::{
    EngineSnapshot, UiState,
    splash::{SPLASH_FADE_SECONDS, SPLASH_HOLD_SECONDS, SPLASH_TOTAL_SECONDS, splash_alpha},
};

const STEP_DT: f32 = 0.1;

fn harness(state: UiState) -> Harness<'static, UiState> {
    let snapshot = EngineSnapshot::default();
    Harness::builder()
        .with_size([1280.0, 800.0])
        .with_step_dt(STEP_DT)
        .with_accessibility_check(false)
        .build_ui_state(
            move |ui, state| {
                modv_ui::shell::draw(ui, &snapshot, state);
            },
            state,
        )
}

#[test]
fn splash_opacity_holds_then_fades_linearly_to_zero() {
    assert_eq!(splash_alpha(0.0), 1.0);
    assert_eq!(splash_alpha(SPLASH_HOLD_SECONDS), 1.0);
    let halfway = SPLASH_HOLD_SECONDS + SPLASH_FADE_SECONDS / 2.0;
    assert!(
        (splash_alpha(halfway) - 0.5).abs() < 1e-6,
        "the logo must be half-faded halfway through the fade"
    );
    assert_eq!(splash_alpha(SPLASH_TOTAL_SECONDS), 0.0);
    assert_eq!(splash_alpha(SPLASH_TOTAL_SECONDS + 10.0), 0.0);
    assert!(
        splash_alpha(halfway) < splash_alpha(SPLASH_HOLD_SECONDS + 0.1),
        "the fade must only ever decrease"
    );
}

#[test]
fn splash_shows_only_the_logo_and_no_about_controls() {
    let mut state = UiState::new();
    state.show_splash();
    let mut harness = harness(state);
    harness.step();

    assert!(
        harness.state().splash.is_some(),
        "requesting the splash must start it"
    );
    assert!(
        harness.state().splash_logo.is_some(),
        "the splash must load the VmodV logo texture"
    );
    for label in ["Adapter:", "Backend:", "Continue", "VmodV splash"] {
        assert!(
            harness.query_all_by_label_contains(label).next().is_none(),
            "the splash must be just the logo — no {label:?} widget"
        );
    }
}

#[test]
fn splash_logo_snapshot() {
    let mut state = UiState::new();
    state.show_splash();
    let mut harness = harness(state);
    harness.step();
    harness.snapshot("splash_logo");
}

#[test]
fn splash_fades_out_and_closes_by_itself() {
    let mut state = UiState::new();
    state.show_splash();
    let mut harness = harness(state);
    harness.step();

    let started = harness
        .state()
        .splash
        .as_ref()
        .and_then(|splash| splash.started_at)
        .expect("the first splash frame must stamp the egui clock");

    // Drive the egui clock to just before the end of the fade. The harness may
    // run several passes in its first frame, so compare against the stamped
    // start time rather than assuming a fixed number of frames.
    while harness.ctx.time() - started < SPLASH_TOTAL_SECONDS - 0.2 {
        harness.step();
    }
    assert!(
        harness.state().splash.is_some(),
        "the splash must stay up until the fade finishes"
    );
    let near_end = harness.state().splash.as_ref().unwrap().started_at.unwrap();
    assert_eq!(near_end, started, "the splash start time must not drift");

    harness.run_steps(3);
    assert!(
        harness.state().splash.is_none(),
        "the splash must close itself once the logo has faded out"
    );
}

#[test]
fn a_fresh_state_does_not_open_the_splash() {
    let mut harness = harness(UiState::new());
    harness.step();

    assert!(harness.state().splash.is_none());
    assert!(harness.state().splash_logo.is_none());
}

#[test]
fn escape_dismisses_the_startup_splash_early() {
    let mut state = UiState::new();
    state.show_splash();
    let mut harness = harness(state);
    harness.step();
    assert!(harness.state().splash.is_some());

    harness.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();

    assert!(
        harness.state().splash.is_none(),
        "Escape must still dismiss the splash early"
    );
}
