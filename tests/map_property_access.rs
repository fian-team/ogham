use ogham::{parser::Parser, scanner::Scanner, vm::Value, vm::VM};

#[test]
fn map_property_access_works() {
    let src = r#"
let main = fn () {
  let m = { a: 1, b: 2 };
  return m.b;
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    let mut vm = VM::new();
    let value = vm.execute_module(&module).expect("vm execution failed");
    assert_eq!(value, Value::Integer(2));
}

#[test]
fn chained_map_property_access_works() {
    let src = r#"
let main = fn () {
  let colors = {
    bg: { r: 18, g: 17, b: 22, a: 255 },
  };
  return colors.bg.r;
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    let mut vm = VM::new();
    let value = vm.execute_module(&module).expect("vm execution failed");
    assert_eq!(value, Value::Integer(18));
}

