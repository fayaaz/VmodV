//! Behavioural tests for the sandboxed Boa plugin host (D5/P7).
//!
//! The plugin sources are bundled here on purpose: the host must not depend on
//! the JavaScript tree, and this module doubles as the loader-contract example.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use modv_engine::plugins::{
    CaptureConfig, FrameCapture, MAX_CAPTURE_DIMENSION, MAX_PLUGIN_SOURCE_BYTES, PluginHost,
    PluginIo, PluginLogLevel, PluginPhase, PluginPropKind, PluginStateReader, PluginTransport,
};
use serde_json::{Value, json};

const COUNTER: &str = r#"
module.exports = {
  name: "Counter",
  author: "modV tests",
  version: "1.0.0",
  props: {
    count: { type: "int", default: 0, min: 0, max: 100, step: 1, abs: true },
    label: { type: "text", default: "hello" },
  },
  init({ props, log }) {
    log("info", "init");
    props.count = props.count + 1;
  },
  preProcessFrame({ props }) {
    props.count = props.count + 1;
  },
  postProcessFrame({ props, width, height, pixels }) {
    if (pixels) {
      props.label = width + "x" + height + ":" + pixels.length;
    } else {
      props.label = "none";
    }
  },
  shutdown({ log }) {
    log("info", "shutdown");
  },
};
"#;

const TOP_LOG: &str = r#"
log("info", "evaluated");
module.exports = {
  name: "TopLog",
  init({ log }) {
    log("info", "init");
  },
  shutdown({ log }) {
    log("info", "shutdown");
  },
};
"#;

const SANDBOX: &str = r#"
module.exports = {
  name: "Sandbox",
  props: { seen: { type: "text", default: "" } },
  preProcessFrame({ props, log }) {
    const types = [
      typeof require,
      typeof process,
      typeof fetch,
      typeof WebSocket,
      typeof XMLHttpRequest,
      typeof module,
      typeof exports,
      typeof console,
      typeof modV.set,
      typeof modV.commit,
      typeof setTimeout,
    ];
    props.seen = types.join(",");
    log("info", props.seen);
  },
};
"#;

const READER: &str = r#"
module.exports = {
  name: "Reader",
  props: {
    bpm: { type: "float", default: 0 },
    missing: { type: "text", default: "" },
  },
  preProcessFrame({ props, modV }) {
    props.bpm = modV.get("beats.bpm");
    props.missing = typeof modV.get("nope.missing");
  },
};
"#;

const SENDER: &str = r#"
module.exports = {
  name: "Sender",
  props: {
    ok: { type: "bool", default: false },
    bad: { type: "bool", default: true },
  },
  preProcessFrame({ props, modV }) {
    props.ok = modV.ws("ws://localhost:3006/modV", { colors: [1, 2, 3] });
    props.bad = modV.ws("http://localhost/nope", "x");
  },
};
"#;

const FAILING_WS: &str = r#"
module.exports = {
  name: "FailingWs",
  props: { ok: { type: "bool", default: true } },
  preProcessFrame({ props, modV }) {
    props.ok = modV.ws("ws://localhost/x", "x");
  },
};
"#;

const CAPTURE: &str = r#"
module.exports = {
  name: "Capture",
  captureWidth: 2,
  captureHeight: 2,
  props: { seen: { type: "text", default: "" } },
  postProcessFrame({ width, height, pixels, props }) {
    if (pixels) {
      props.seen = width + "x" + height + ":" + pixels.length + ":" + pixels[0];
    } else {
      props.seen = "none";
    }
  },
};
"#;

const BIG_CAPTURE: &str = r#"
module.exports = {
  name: "BigCapture",
  captureWidth: 3,
  captureHeight: 5,
  props: { seen: { type: "text", default: "" } },
  postProcessFrame({ width, height, pixels, props }) {
    props.seen = width + "x" + height + ":" + pixels.length;
  },
};
"#;

const CAPTURE_OBJECT: &str = r#"
module.exports = {
  name: "CaptureObject",
  captureWidth: 1,
  captureHeight: 1,
  props: { ok: { type: "bool", default: false } },
  postProcessFrame({ width, height, pixels, capture, props }) {
    props.ok =
      width === 1 &&
      height === 1 &&
      pixels.length === 4 &&
      capture.width === 1 &&
      capture.height === 1 &&
      capture.pixels === pixels;
  },
};
"#;

const PROPS: &str = r#"
module.exports = {
  name: "Props",
  props: {
    i: { type: "int", default: 50, min: 0, max: 10 },
    f: { type: "float", default: -3, abs: true },
    b: { type: "bool", default: false },
    t: { type: "text", default: "x" },
    c: { type: "color", default: { r: 0.5, g: 0.6, b: 0.7, a: 1 } },
    s: {
      type: "select",
      default: "b",
      options: [
        { label: "A", value: "a" },
        { label: "B", value: "b" },
      ],
    },
  },
  preProcessFrame({ props }) {
    props.i = 999;
    props.f = -2.5;
    props.b = 1;
    props.s = "zzz";
  },
};
"#;

const THROW_PRE: &str = r#"
module.exports = {
  name: "ThrowPre",
  preProcessFrame() {
    throw new Error("boom");
  },
};
"#;

const THROW_INIT: &str = r#"
module.exports = {
  name: "ThrowInit",
  init() {
    throw new Error("no init");
  },
};
"#;

const GOOD: &str = r#"
module.exports = {
  name: "Good",
  props: { n: { type: "int", default: 0 } },
  preProcessFrame({ props }) {
    props.n = props.n + 1;
  },
};
"#;

const RUNAWAY: &str = r#"
module.exports = {
  name: "Runaway",
  preProcessFrame() {
    while (true) {}
  },
};
"#;

const GLOBAL_FALLBACK: &str = r#"
globalThis.modVPlugin = {
  name: "Global",
  props: { ok: { type: "bool", default: true } },
};
"#;

const DEFAULT_FALLBACK: &str = r#"
module.exports = {
  default: {
    name: "Defaulted",
    props: { ok: { type: "bool", default: true } },
  },
};
"#;

#[derive(Default)]
struct FakeState {
    values: HashMap<String, Value>,
    calls: RefCell<Vec<String>>,
}

impl FakeState {
    fn with(values: &[(&str, Value)]) -> Self {
        Self {
            values: values
                .iter()
                .map(|(path, value)| ((*path).to_owned(), value.clone()))
                .collect(),
            calls: RefCell::default(),
        }
    }
}

impl PluginStateReader for FakeState {
    fn get(&self, path: &str) -> Option<Value> {
        self.calls.borrow_mut().push(path.to_owned());
        self.values.get(path).cloned()
    }
}

#[derive(Default)]
struct FakeTransport {
    sent: Vec<(String, String)>,
    fail: bool,
}

impl PluginTransport for FakeTransport {
    fn post(&mut self, url: &str, payload: &str) -> Result<(), String> {
        if self.fail {
            return Err("offline".to_owned());
        }
        self.sent.push((url.to_owned(), payload.to_owned()));
        Ok(())
    }
}

fn host_with(sources: &[&str]) -> PluginHost {
    let mut host = PluginHost::new(false);
    for source in sources {
        host.add_source(*source).expect("source registers");
    }
    host
}

type SharedState = Rc<FakeState>;
type SharedTransport = Rc<RefCell<FakeTransport>>;

fn enable(host: &mut PluginHost) -> (SharedState, SharedTransport, Value) {
    let features = json!({ "bass": 0.25, "mid": 0.5, "treble": 0.75 });
    let state = Rc::new(FakeState::default());
    let transport = Rc::new(RefCell::new(FakeTransport::default()));
    let mut io = PluginIo::new(features.clone(), state.clone(), transport.clone());
    host.set_enabled(true, &mut io);
    (state, transport, features)
}

fn run_frame(
    host: &mut PluginHost,
    state: &SharedState,
    transport: &SharedTransport,
    features: &Value,
    capture: Option<&FrameCapture>,
) {
    let mut io = PluginIo::new(features.clone(), state.clone(), transport.clone());
    host.pre_frame(&mut io);
    host.post_frame(&mut io, capture).expect("post frame");
}

fn loaded_count(host: &PluginHost) -> usize {
    host.plugins().iter().filter(|plugin| plugin.loaded).count()
}

fn count_logs(host: &PluginHost, message: &str) -> usize {
    host.logs()
        .iter()
        .filter(|log| log.message == message)
        .count()
}

#[test]
fn a_disabled_host_allocates_no_context_and_calls_no_js() {
    let mut host = host_with(&[TOP_LOG]);
    assert!(!host.enabled());
    assert_eq!(loaded_count(&host), 0);
    assert!(host.logs().is_empty());
    assert!(host.errors().is_empty());

    let mut io = PluginIo::empty();
    host.pre_frame(&mut io);
    host.post_frame(&mut io, None).expect("post frame");

    assert_eq!(loaded_count(&host), 0);
    assert!(host.logs().is_empty());
}

#[test]
fn enabling_loads_and_initialises_exactly_once() {
    let mut host = host_with(&[TOP_LOG]);
    let (_state, _transport, _features) = enable(&mut host);

    assert!(host.enabled());
    assert_eq!(loaded_count(&host), 1);
    assert_eq!(count_logs(&host, "evaluated"), 1);
    assert_eq!(count_logs(&host, "init"), 1);

    // A second enable must not re-load or re-init the running context.
    let mut io = PluginIo::empty();
    host.set_enabled(true, &mut io);
    assert_eq!(count_logs(&host, "evaluated"), 1);
    assert_eq!(count_logs(&host, "init"), 1);
}

#[test]
fn pre_and_post_hooks_receive_the_documented_shapes() {
    let mut host = host_with(&[COUNTER]);
    let (state, transport, features) = enable(&mut host);

    // init increments the counter once; the first pre frame once more.
    run_frame(&mut host, &state, &transport, &features, None);
    let info = &host.plugins()[0];
    assert_eq!(info.props["count"].value, json!(2));
    assert_eq!(info.props["label"].value, json!("none"));

    let capture = FrameCapture::new(2, 2, vec![7; 16]).expect("valid capture");
    run_frame(&mut host, &state, &transport, &features, Some(&capture));
    let info = &host.plugins()[0];
    assert_eq!(info.props["count"].value, json!(3));
    assert_eq!(info.props["label"].value, json!("2x2:16"));
}

#[test]
fn sandbox_globals_are_absent() {
    let mut host = host_with(&[SANDBOX]);
    let (state, transport, features) = enable(&mut host);
    run_frame(&mut host, &state, &transport, &features, None);

    let seen = host.plugins()[0].props["seen"]
        .value
        .as_str()
        .expect("seen is text")
        .to_owned();
    let types: Vec<&str> = seen.split(',').collect();
    assert_eq!(types.len(), 11, "{seen}");
    for name in [
        "require",
        "process",
        "fetch",
        "WebSocket",
        "XMLHttpRequest",
        "module",
        "exports",
        "console",
        "modV.set",
        "modV.commit",
        "setTimeout",
    ] {
        assert!(
            seen.split(',').all(|value| value == "undefined"),
            "{name} leaked into the sandbox: {seen}"
        );
    }
}

#[test]
fn modv_get_reads_through_the_injected_state() {
    let mut host = host_with(&[READER]);
    let features = json!({});
    let state = Rc::new(FakeState::with(&[("beats.bpm", json!(128.0))]));
    let transport = Rc::new(RefCell::new(FakeTransport::default()));

    let mut io = PluginIo::new(features, state.clone(), transport);
    host.set_enabled(true, &mut io);
    host.pre_frame(&mut io);

    let info = &host.plugins()[0];
    assert_eq!(info.props["bpm"].value, json!(128.0));
    assert_eq!(info.props["missing"].value, json!("undefined"));
    assert_eq!(&*state.calls.borrow(), &["beats.bpm", "nope.missing"]);
}

#[test]
fn modv_ws_posts_through_the_injected_transport() {
    let mut host = host_with(&[SENDER]);
    let (state, transport, features) = enable(&mut host);
    run_frame(&mut host, &state, &transport, &features, None);

    assert_eq!(
        &transport.borrow().sent,
        &[(
            "ws://localhost:3006/modV".to_owned(),
            r#"{"colors":[1,2,3]}"#.to_owned()
        )]
    );
    let info = &host.plugins()[0];
    assert_eq!(info.props["ok"].value, json!(true));
    assert_eq!(info.props["bad"].value, json!(false));
}

#[test]
fn transport_failures_are_reported_once_and_do_not_stop_the_hook() {
    let mut host = host_with(&[FAILING_WS]);
    let features = json!({});
    let state = Rc::new(FakeState::default());
    let transport = Rc::new(RefCell::new(FakeTransport {
        fail: true,
        ..Default::default()
    }));
    let mut io = PluginIo::new(features, state, transport);
    host.set_enabled(true, &mut io);
    host.pre_frame(&mut io);
    host.pre_frame(&mut io);

    assert_eq!(host.plugins()[0].props["ok"].value, json!(false));
    let transport_errors: Vec<_> = host
        .errors()
        .into_iter()
        .filter(|error| error.phase == PluginPhase::Transport)
        .collect();
    assert_eq!(transport_errors.len(), 1, "{:?}", host.errors());
}

#[test]
fn capture_is_validated_capped_and_downscaled() {
    assert!(FrameCapture::new(0, 1, Vec::new()).is_err());
    assert!(FrameCapture::new(2, 2, vec![0; 15]).is_err());
    assert!(FrameCapture::new(MAX_CAPTURE_DIMENSION + 1, 1, vec![0; 4]).is_err());

    let capture = FrameCapture::new(
        2,
        2,
        vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 255, // white
        ],
    )
    .expect("valid capture");
    let downscaled = capture.downscale(1, 1).expect("downscale");
    assert_eq!(downscaled.width(), 1);
    assert_eq!(downscaled.height(), 1);
    assert_eq!(downscaled.pixels(), &[127, 127, 127, 255]);
    assert!(capture.downscale(0, 1).is_err());

    let mut host = host_with(&[CAPTURE]);
    let (state, transport, features) = enable(&mut host);
    assert_eq!(
        host.capture_config(),
        Some(CaptureConfig {
            width: 2,
            height: 2
        })
    );

    run_frame(&mut host, &state, &transport, &features, None);
    assert_eq!(host.plugins()[0].props["seen"].value, json!("none"));

    let capture = FrameCapture::new(2, 2, vec![7; 16]).expect("valid capture");
    run_frame(&mut host, &state, &transport, &features, Some(&capture));
    assert_eq!(host.plugins()[0].props["seen"].value, json!("2x2:16:7"));

    let oversized_pixels = vec![0; ((MAX_CAPTURE_DIMENSION + 1) * 4) as usize];
    let oversized = FrameCapture::new(MAX_CAPTURE_DIMENSION + 1, 1, oversized_pixels);
    assert!(oversized.is_err());
}

#[test]
fn prop_defaults_are_coerced_and_clamped() {
    let mut host = host_with(&[PROPS]);
    let (state, transport, features) = enable(&mut host);
    let id = host.plugins()[0].id;

    {
        let info = &host.plugins()[0];
        assert_eq!(info.props["i"].kind, PluginPropKind::Int);
        assert_eq!(info.props["i"].value, json!(10));
        assert_eq!(info.props["f"].value, json!(3.0));
        assert_eq!(info.props["b"].value, json!(false));
        assert_eq!(info.props["t"].value, json!("x"));
        assert_eq!(
            info.props["c"].value,
            json!({"r":0.5,"g":0.6,"b":0.7,"a":1.0})
        );
        assert_eq!(info.props["s"].value, json!("b"));
        assert_eq!(info.props["s"].options.len(), 2);
    }

    run_frame(&mut host, &state, &transport, &features, None);
    {
        let info = &host.plugins()[0];
        assert_eq!(info.props["i"].value, json!(10));
        assert_eq!(info.props["f"].value, json!(2.5));
        assert_eq!(info.props["b"].value, json!(true));
        assert_eq!(info.props["s"].value, json!("b"));
    }

    assert_eq!(host.set_prop(id, "i", json!(3.7)).expect("set"), json!(4));
    assert_eq!(
        host.set_prop(id, "f", json!(-9.0)).expect("set"),
        json!(9.0)
    );
    assert_eq!(host.set_prop(id, "s", json!("a")).expect("set"), json!("a"));
    assert!(host.set_prop(id, "missing", json!(1)).is_err());
}

#[test]
fn one_throwing_plugin_does_not_stop_the_others_and_is_reported_once() {
    let mut host = host_with(&[THROW_PRE, GOOD]);
    let (state, transport, features) = enable(&mut host);
    assert_eq!(loaded_count(&host), 2);

    run_frame(&mut host, &state, &transport, &features, None);
    run_frame(&mut host, &state, &transport, &features, None);

    assert_eq!(host.plugins()[1].props["n"].value, json!(2));
    let pre_errors: Vec<_> = host
        .errors()
        .into_iter()
        .filter(|error| error.phase == PluginPhase::PreFrame)
        .collect();
    assert_eq!(pre_errors.len(), 1, "{:?}", host.errors());
    assert_eq!(pre_errors[0].plugin_name.as_deref(), Some("ThrowPre"));
}

#[test]
fn a_plugin_that_throws_in_init_is_disabled_without_taking_the_host_down() {
    let mut host = host_with(&[THROW_INIT, GOOD]);
    let (_state, _transport, _features) = enable(&mut host);

    let plugins = host.plugins();
    assert_eq!(plugins.len(), 2);
    assert!(!plugins[0].loaded);
    assert!(plugins[0].failed);
    assert_eq!(plugins[0].name, "ThrowInit");
    assert!(plugins[1].loaded);
    assert!(
        host.errors()
            .iter()
            .any(|error| error.phase == PluginPhase::Init),
        "{:?}",
        host.errors()
    );
}

#[test]
fn invalid_and_duplicate_names_are_rejected_independently() {
    let mut host = host_with(&[
        "module.exports = { name: 'Same' };",
        "module.exports = { name: 'same' };",
        "module.exports = { name: '' };",
        "module.exports = { nope: true };",
        "module.exports = 42;",
        "module.exports = {",
        GLOBAL_FALLBACK,
        DEFAULT_FALLBACK,
    ]);
    let (_state, _transport, _features) = enable(&mut host);

    let plugins = host.plugins();
    assert_eq!(loaded_count(&host), 3, "{plugins:#?}");
    assert!(plugins[0].loaded);
    assert!(!plugins[1].loaded);
    assert!(!plugins[2].loaded);
    assert!(!plugins[3].loaded);
    assert!(!plugins[4].loaded);
    assert!(!plugins[5].loaded);
    assert_eq!(plugins[6].name, "Global");
    assert!(plugins[6].loaded);
    assert_eq!(plugins[7].name, "Defaulted");
    assert!(plugins[7].loaded);
    assert!(
        host.errors()
            .iter()
            .filter(|error| error.phase == PluginPhase::Load)
            .count()
            >= 5,
        "{:?}",
        host.errors()
    );
}

#[test]
fn a_runaway_loop_is_stopped_by_the_runtime_budget() {
    let mut host = host_with(&[RUNAWAY, GOOD]);
    let (state, transport, features) = enable(&mut host);
    run_frame(&mut host, &state, &transport, &features, None);

    assert_eq!(host.plugins()[1].props["n"].value, json!(1));
    assert!(
        host.errors()
            .iter()
            .any(|error| error.phase == PluginPhase::PreFrame
                && error.plugin_name.as_deref() == Some("Runaway")),
        "{:?}",
        host.errors()
    );
}

#[test]
fn disabling_runs_shutdown_and_drops_the_context() {
    let mut host = host_with(&[TOP_LOG]);
    let (state, transport, features) = enable(&mut host);
    assert_eq!(count_logs(&host, "init"), 1);

    let mut io = PluginIo::new(features, state, transport);
    host.set_enabled(false, &mut io);
    assert!(!host.enabled());
    assert_eq!(count_logs(&host, "shutdown"), 1);
    assert_eq!(loaded_count(&host), 0);
    assert!(host.capture_config().is_none());

    host.pre_frame(&mut io);
    assert_eq!(count_logs(&host, "evaluated"), 1);

    host.set_enabled(true, &mut io);
    assert_eq!(count_logs(&host, "evaluated"), 2);
    assert_eq!(count_logs(&host, "init"), 2);
}

#[test]
fn registration_order_is_preserved() {
    let first = r#"
module.exports = {
  name: "First",
  preProcessFrame({ log }) { log("info", "first"); },
};
"#;
    let second = r#"
module.exports = {
  name: "Second",
  preProcessFrame({ log }) { log("info", "second"); },
};
"#;
    let mut host = host_with(&[first, second]);
    let (state, transport, features) = enable(&mut host);
    run_frame(&mut host, &state, &transport, &features, None);

    let order: Vec<String> = host
        .logs()
        .into_iter()
        .filter(|log| log.level == PluginLogLevel::Info)
        .map(|log| log.message)
        .collect();
    assert_eq!(order, ["first", "second"]);
}

#[test]
fn properties_written_by_the_plugin_are_clamped_before_the_next_hook() {
    let source = r#"
module.exports = {
  name: "Writer",
  props: { n: { type: "int", default: 0, min: 0, max: 5 } },
  preProcessFrame({ props }) {
    props.n = props.n + 100;
  },
};
"#;
    let mut host = host_with(&[source]);
    let (state, transport, features) = enable(&mut host);
    run_frame(&mut host, &state, &transport, &features, None);
    assert_eq!(host.plugins()[0].props["n"].value, json!(5));
    run_frame(&mut host, &state, &transport, &features, None);
    // The clamp is visible to the plugin on the next frame, so this does not
    // run away: 5 + 100 clamps back to 5.
    assert_eq!(host.plugins()[0].props["n"].value, json!(5));
}

#[test]
fn prop_errors_before_loading_are_reported_not_panicked() {
    let mut host = PluginHost::new(false);
    let id = host.add_source(COUNTER).expect("source registers");
    // The descriptor is not parsed until the host is enabled, so the prop is
    // unknown here and the host reports an error instead of panicking.
    let error = host
        .set_prop(id, "count", json!(1))
        .expect_err("prop is unknown before load");
    assert_eq!(error.phase, PluginPhase::Props);
}

#[test]
fn commonjs_exports_alias_loads_a_plugin() {
    let source = r#"
exports.name = "ExportsAlias";
exports.props = { ok: { type: "bool", default: true } };
exports.preProcessFrame = function ({ props }) { props.ok = false; };
"#;
    let mut host = host_with(&[source]);
    let (state, transport, features) = enable(&mut host);
    assert_eq!(host.plugins()[0].name, "ExportsAlias");
    assert!(host.plugins()[0].loaded);
    run_frame(&mut host, &state, &transport, &features, None);
    assert_eq!(host.plugins()[0].props["ok"].value, json!(false));
}

#[test]
fn per_plugin_controls_drop_and_reload_only_that_context() {
    let mut host = host_with(&[COUNTER]);
    let (state, transport, features) = enable(&mut host);
    let id = host.plugins()[0].id;
    let mut io = PluginIo::new(features, state, transport);

    host.set_plugin_enabled(id, false, &mut io)
        .expect("disable plugin");
    assert!(!host.plugins()[0].active);
    assert!(!host.plugins()[0].loaded);
    assert_eq!(count_logs(&host, "shutdown"), 1);

    host.set_plugin_enabled(id, true, &mut io)
        .expect("enable plugin");
    assert!(host.plugins()[0].loaded);
    assert_eq!(count_logs(&host, "init"), 2);

    host.reload(id, &mut io).expect("reload plugin");
    assert!(host.plugins()[0].loaded);
    assert_eq!(count_logs(&host, "init"), 3);

    assert!(host.remove(id, &mut io));
    assert!(host.plugins().is_empty());
}

#[test]
fn adding_a_source_to_an_enabled_host_can_load_immediately() {
    let mut host = PluginHost::new(true);
    let mut io = PluginIo::empty();
    host.add_source_with_io(COUNTER, &mut io)
        .expect("source loads");
    assert_eq!(loaded_count(&host), 1);
    assert_eq!(count_logs(&host, "init"), 1);
}

#[test]
fn capture_config_is_the_max_and_each_plugin_gets_its_own_size() {
    let mut host = host_with(&[CAPTURE, BIG_CAPTURE, CAPTURE_OBJECT]);
    let (state, transport, features) = enable(&mut host);
    assert_eq!(
        host.capture_config(),
        Some(CaptureConfig {
            width: 3,
            height: 5
        })
    );

    let capture = FrameCapture::new(3, 5, vec![9; 60]).expect("valid capture");
    run_frame(&mut host, &state, &transport, &features, Some(&capture));

    let plugins = host.plugins();
    assert_eq!(plugins[0].props["seen"].value, json!("2x2:16:9"));
    assert_eq!(plugins[1].props["seen"].value, json!("3x5:60"));
    assert_eq!(plugins[2].props["ok"].value, json!(true));
}

#[test]
fn huge_sources_and_malformed_descriptors_are_rejected_per_plugin() {
    let mut host = PluginHost::new(false);
    let huge = " ".repeat(MAX_PLUGIN_SOURCE_BYTES + 1);
    assert!(host.add_source(huge).is_err());

    host.add_source("module.exports = function () {};")
        .expect("source registers");
    host.add_source("module.exports = { name: 'BadHook', init: 42 };")
        .expect("source registers");
    host.add_source("module.exports = { name: 'BadProp', props: { x: { type: 'nope' } } };")
        .expect("source registers");
    host.add_source(
        "module.exports = { name: 'Proto', props: { ['__proto__']: { type: 'text', default: 'x' } } };",
    )
    .expect("source registers");
    host.add_source(GOOD).expect("source registers");

    let mut io = PluginIo::empty();
    host.set_enabled(true, &mut io);

    let plugins = host.plugins();
    assert!(!plugins[0].loaded);
    assert!(!plugins[1].loaded);
    assert!(!plugins[2].loaded);
    assert!(!plugins[3].loaded);
    assert!(plugins[4].loaded);
    assert!(
        host.errors()
            .iter()
            .filter(|error| error.phase == PluginPhase::Load)
            .count()
            >= 4,
        "{:?}",
        host.errors()
    );
}

#[test]
fn a_top_level_runaway_loop_is_stopped_during_load() {
    let mut host = host_with(&["while (true) {}", GOOD]);
    let (_state, _transport, _features) = enable(&mut host);

    assert!(!host.plugins()[0].loaded);
    assert!(host.plugins()[1].loaded);
    assert!(
        host.errors()
            .iter()
            .any(|error| error.phase == PluginPhase::Load),
        "{:?}",
        host.errors()
    );
}

#[test]
fn drain_logs_takes_the_collected_lines() {
    let mut host = host_with(&[TOP_LOG]);
    let (_state, _transport, _features) = enable(&mut host);

    let drained = host.drain_logs();
    assert_eq!(drained.len(), 2);
    assert!(host.logs().is_empty());
    assert!(host.drain_logs().is_empty());
}
