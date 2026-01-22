use ogham::runtime::RuntimeConfig;
use ogham::tree::event::Event;
use ogham::tree::point::Point;
use ogham::runtime::Value;
use std::sync::{Arc, Mutex};

#[test]
fn flex_mouse_down_calls_ogham_function_and_can_emit_host_event() {
    let src = r#"
let main = fn () {
  return Flex {
    style: { width: 100, height: 100 },
    mouse_down: fn () {
      event("hit", 42);
    },
    children: [
      Text { text: "x" }
    ],
  };
};
"#;

    let calls: Arc<Mutex<Vec<Vec<Value>>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_for_handler = calls.clone();

    let config = RuntimeConfig::new().with_event_handler("hit", move |args| {
        calls_for_handler.lock().unwrap().push(args.to_vec());
        true
    });

    let mut ui = ogham::runtime::from_source(src, Some(config)).expect("compile failed");
    ui.layout(100.0, 100.0);

    let handled = ui.call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(1.0, 1.0),
    ));
    assert!(handled, "expected mouse_down to be handled");

    let got = calls.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], vec![Value::Integer(42)]);
}

