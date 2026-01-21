use ogham::{parser::Parser, scanner::Scanner, vm::Value, vm::VM};
use std::sync::{Arc, Mutex};

#[test]
fn event_call_dispatches_only_when_registered_and_passes_argument_list() {
    let src = r#"
let main = fn () {
  event("unregistered", 1);
  event("registered", "hello", 2, true);
  event("registered");
  return 0;
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    let calls: Arc<Mutex<Vec<(String, Vec<Value>)>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_for_handler = calls.clone();

    let mut vm = VM::new();
    vm.register_event_handler("registered", move |args| {
        calls_for_handler
            .lock()
            .unwrap()
            .push(("registered".to_string(), args.to_vec()));
        true
    });

    let value = vm.execute_module(&module).expect("vm execution failed");
    assert_eq!(value, Value::Integer(0));

    let got = calls.lock().unwrap().clone();
    assert_eq!(got.len(), 2);

    assert_eq!(got[0].0, "registered");
    assert_eq!(
        got[0].1,
        vec![
            Value::String("hello".to_string()),
            Value::Integer(2),
            Value::Boolean(true),
        ]
    );

    assert_eq!(got[1].0, "registered");
    assert!(got[1].1.is_empty());
}

