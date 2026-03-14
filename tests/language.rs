//! Comprehensive language conformance test suite for Ogham.

mod common;

use std::collections::HashMap;

use common::{eval, run};
use ogham::runtime::value::Value;

// ===========================================================================
// Variables and state
// ===========================================================================

#[test]
fn let_integer() {
    assert_eq!(eval("let x = 42; x"), Value::Integer(42));
}

#[test]
fn let_float() {
    assert_eq!(eval("let x = 3.14; x"), Value::Float(3.14));
}

#[test]
fn let_string() {
    assert_eq!(
        eval(r#"let x = "hello"; x"#),
        Value::String("hello".to_string())
    );
}

#[test]
fn let_boolean_true() {
    assert_eq!(eval("let x = true; x"), Value::Boolean(true));
}

#[test]
fn let_boolean_false() {
    assert_eq!(eval("let x = false; x"), Value::Boolean(false));
}

#[test]
fn state_persists_across_rerenders() {
    let source = r#"
let main = fn () {
    state count = 0;
    count = count + 1;
    count
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    let first = runtime.execute_module(&module).expect("first execute");
    assert_eq!(first, Value::Integer(1));
    let second = runtime.rerender().expect("rerender");
    assert_eq!(second, Value::Integer(2));
    let third = runtime.rerender().expect("rerender 2");
    assert_eq!(third, Value::Integer(3));
}

#[test]
fn variable_shadowing() {
    let source = r#"
let main = fn () {
    let x = 1;
    let inner = 99;
    if (x == 1) {
        return inner;
    } else {
        return x;
    }
};
"#;
    assert_eq!(run(source), Value::Integer(99));
}

#[test]
fn variable_assignment() {
    assert_eq!(eval("let x = 1; x = 10; x"), Value::Integer(10));
}

// ===========================================================================
// Arithmetic operators
// ===========================================================================

#[test]
fn add_integers() {
    assert_eq!(eval("2 + 3"), Value::Integer(5));
}

#[test]
fn subtract_integers() {
    assert_eq!(eval("10 - 4"), Value::Integer(6));
}

#[test]
fn multiply_integers() {
    assert_eq!(eval("3 * 7"), Value::Integer(21));
}

#[test]
fn divide_integers_produces_float() {
    assert_eq!(eval("10 / 4"), Value::Float(2.5));
}

#[test]
fn add_floats() {
    assert_eq!(eval("1.5 + 2.5"), Value::Float(4.0));
}

#[test]
fn subtract_floats() {
    assert_eq!(eval("5.0 - 1.5"), Value::Float(3.5));
}

#[test]
fn multiply_floats() {
    assert_eq!(eval("2.0 * 3.0"), Value::Float(6.0));
}

#[test]
fn divide_floats() {
    assert_eq!(eval("7.0 / 2.0"), Value::Float(3.5));
}

#[test]
fn mixed_int_float_add() {
    assert_eq!(eval("1 + 2.5"), Value::Float(3.5));
}

#[test]
fn mixed_float_int_subtract() {
    assert_eq!(eval("5.5 - 2"), Value::Float(3.5));
}

#[test]
fn modulo_integers() {
    assert_eq!(eval("10 % 3"), Value::Integer(1));
}

#[test]
fn modulo_floats() {
    assert_eq!(eval("10.5 % 3.0"), Value::Float(1.5));
}

#[test]
fn power_integers() {
    assert_eq!(eval("2 ^ 10"), Value::Integer(1024));
}

#[test]
fn power_floats() {
    assert_eq!(eval("4.0 ^ 0.5"), Value::Float(2.0));
}

#[test]
#[should_panic]
fn division_by_zero_errors() {
    eval("1 / 0");
}

#[test]
#[should_panic]
fn modulo_by_zero_errors() {
    eval("10 % 0");
}

#[test]
fn string_concatenation() {
    assert_eq!(
        eval(r#""hello" + " " + "world""#),
        Value::String("hello world".to_string())
    );
}

#[test]
fn string_concat_with_int() {
    assert_eq!(
        eval(r#""count: " + 42"#),
        Value::String("count: 42".to_string())
    );
}

#[test]
fn precedence_add_mul() {
    assert_eq!(eval("2 + 3 * 4"), Value::Integer(14));
}

#[test]
fn precedence_power_mul() {
    assert_eq!(eval("2 ^ 3 * 2"), Value::Integer(16));
}

// ===========================================================================
// Unary operators
// ===========================================================================

#[test]
fn negate_integer() {
    assert_eq!(eval("-5"), Value::Integer(-5));
}

#[test]
fn negate_float() {
    assert_eq!(eval("-3.14"), Value::Float(-3.14));
}

#[test]
fn logical_not_true() {
    assert_eq!(eval("!true"), Value::Boolean(false));
}

#[test]
fn logical_not_false() {
    assert_eq!(eval("!false"), Value::Boolean(true));
}

#[test]
fn prefix_increment() {
    assert_eq!(eval("let x = 5; ++x"), Value::Integer(6));
}

#[test]
fn prefix_increment_updates_variable() {
    assert_eq!(eval("let x = 5; ++x; x"), Value::Integer(6));
}

#[test]
fn postfix_increment_returns_old_value() {
    let source = r#"
let main = fn () {
    let x = 5;
    let old = x++;
    return old;
};
"#;
    assert_eq!(run(source), Value::Integer(5));
}

#[test]
fn postfix_increment_updates_variable() {
    let source = r#"
let main = fn () {
    let x = 5;
    let _ = x++;
    return x;
};
"#;
    assert_eq!(run(source), Value::Integer(6));
}

// ===========================================================================
// Comparison operators
// ===========================================================================

#[test]
fn eq_integers() {
    assert_eq!(eval("3 == 3"), Value::Boolean(true));
}

#[test]
fn eq_integers_false() {
    assert_eq!(eval("3 == 4"), Value::Boolean(false));
}

#[test]
fn ne_integers() {
    assert_eq!(eval("3 != 4"), Value::Boolean(true));
}

#[test]
fn eq_strings() {
    assert_eq!(eval(r#""abc" == "abc""#), Value::Boolean(true));
}

#[test]
fn ne_strings() {
    assert_eq!(eval(r#""abc" != "def""#), Value::Boolean(true));
}

#[test]
fn eq_booleans() {
    assert_eq!(eval("true == true"), Value::Boolean(true));
}

#[test]
fn ne_booleans() {
    assert_eq!(eval("true != false"), Value::Boolean(true));
}

#[test]
fn greater_than_int() {
    assert_eq!(eval("5 > 3"), Value::Boolean(true));
}

#[test]
fn greater_than_or_equal_int() {
    assert_eq!(eval("5 >= 5"), Value::Boolean(true));
}

#[test]
fn less_than_int() {
    assert_eq!(eval("2 < 10"), Value::Boolean(true));
}

#[test]
fn less_than_or_equal_int() {
    assert_eq!(eval("3 <= 3"), Value::Boolean(true));
}

#[test]
fn greater_than_float() {
    assert_eq!(eval("3.5 > 2.1"), Value::Boolean(true));
}

#[test]
fn less_than_float() {
    assert_eq!(eval("1.1 < 2.2"), Value::Boolean(true));
}

// ===========================================================================
// Logical operators
// ===========================================================================

#[test]
fn and_true_true() {
    assert_eq!(eval("true && true"), Value::Boolean(true));
}

#[test]
fn and_true_false() {
    assert_eq!(eval("true && false"), Value::Boolean(false));
}

#[test]
fn or_false_true() {
    assert_eq!(eval("false || true"), Value::Boolean(true));
}

#[test]
fn or_false_false() {
    assert_eq!(eval("false || false"), Value::Boolean(false));
}

// ===========================================================================
// Control flow
// ===========================================================================

#[test]
fn if_true_branch() {
    assert_eq!(eval("if true { 1 } else { 2 }"), Value::Integer(1));
}

#[test]
fn if_false_branch() {
    assert_eq!(eval("if false { 1 } else { 2 }"), Value::Integer(2));
}

#[test]
fn if_else_if_chain() {
    let source = r#"
let main = fn () {
    let x = 2;
    if (x == 1) {
        return "one";
    } else if (x == 2) {
        return "two";
    } else {
        return "other";
    }
};
"#;
    assert_eq!(run(source), Value::String("two".to_string()));
}

#[test]
fn if_with_boolean_variable() {
    let source = r#"
let main = fn () {
    let flag = true;
    if (flag) {
        return 1;
    } else {
        return 0;
    }
};
"#;
    assert_eq!(run(source), Value::Integer(1));
}

#[test]
fn match_integer_patterns() {
    let source = r#"
let main = fn () {
    let x = 3;
    let result = match x {
        1 => "one",
        2 => "two",
        3 => "three",
        _ => "other",
    };
    result
};
"#;
    assert_eq!(run(source), Value::String("three".to_string()));
}

#[test]
fn match_string_patterns() {
    let source = r#"
let main = fn () {
    let s = "hi";
    let result = match s {
        "hello" => 1,
        "hi" => 2,
        _ => 0,
    };
    result
};
"#;
    assert_eq!(run(source), Value::Integer(2));
}

#[test]
fn match_boolean_patterns() {
    let source = r#"
let main = fn () {
    let b = false;
    let result = match b {
        true => "yes",
        false => "no",
    };
    result
};
"#;
    assert_eq!(run(source), Value::String("no".to_string()));
}

#[test]
fn match_wildcard() {
    let source = r#"
let main = fn () {
    let x = 999;
    let result = match x {
        1 => "one",
        _ => "wildcard",
    };
    result
};
"#;
    assert_eq!(run(source), Value::String("wildcard".to_string()));
}

#[test]
fn for_loop_statement_side_effects() {
    let source = r#"
let main = fn () {
    state total = 0;
    for (i in 0..5) {
        total = total + i;
    }
    total
};
"#;
    assert_eq!(run(source), Value::Integer(10));
}

#[test]
fn for_loop_expression_produces_array() {
    let source = r#"
let main = fn () {
    return for (i in 0..4) { (i * 2) };
};
"#;
    assert_eq!(
        run(source),
        Value::Array(vec![
            Value::Integer(0),
            Value::Integer(2),
            Value::Integer(4),
            Value::Integer(6),
        ])
    );
}

// ===========================================================================
// Functions
// ===========================================================================

#[test]
fn function_definition_and_call() {
    let source = r#"
let double = fn (x: int): int { return x * 2; };
let main = fn () { double(5) };
"#;
    assert_eq!(run(source), Value::Integer(10));
}

#[test]
fn function_multiple_parameters() {
    let source = r#"
let add = fn (a: int, b: int): int { return a + b; };
let main = fn () { add(3, 7) };
"#;
    assert_eq!(run(source), Value::Integer(10));
}

#[test]
fn higher_order_function() {
    let source = r#"
let apply = fn (f: closure, x: int): int { return f(x); };
let double = fn (n: int): int { return n * 2; };
let main = fn () { apply(double, 7) };
"#;
    assert_eq!(run(source), Value::Integer(14));
}

#[test]
#[should_panic(expected = "UndefinedVariable")]
fn recursive_function_not_supported() {
    let source = r#"
let fact = fn (n: int): int {
    if (n <= 1) { return 1; } else { return n * fact(n - 1); }
};
let main = fn () { fact(5) };
"#;
    run(source);
}

#[test]
fn implicit_return() {
    let source = r#"
let greet = fn (name: string): string { "Hello, " + name };
let main = fn () { greet("Ogham") };
"#;
    assert_eq!(run(source), Value::String("Hello, Ogham".to_string()));
}

#[test]
fn closure_captures_variable() {
    let source = r#"
let make_adder = fn (x: int) {
    return fn (y: int): int { return x + y; };
};
let main = fn () {
    let add10 = make_adder(10);
    add10(5)
};
"#;
    assert_eq!(run(source), Value::Integer(15));
}

#[test]
fn nested_function_calls() {
    let source = r#"
let double = fn (n: int): int { return n * 2; };
let main = fn () { double(double(3)) };
"#;
    assert_eq!(run(source), Value::Integer(12));
}

// ===========================================================================
// Data structures
// ===========================================================================

#[test]
fn map_literal() {
    let source = r#"
let main = fn () {
    let m = { x: 10, y: 20 };
    return m.x + m.y;
};
"#;
    assert_eq!(run(source), Value::Integer(30));
}

#[test]
fn map_property_access() {
    let source = r#"
let main = fn () {
    let person = { name: "Alice", age: 30 };
    return person.name;
};
"#;
    assert_eq!(run(source), Value::String("Alice".to_string()));
}

#[test]
fn array_literal() {
    assert_eq!(
        eval("[1, 2, 3]"),
        Value::Array(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ])
    );
}

#[test]
fn array_index_access() {
    let source = r#"
let main = fn () {
    let arr = [10, 20, 30];
    let v = arr[1];
    return v;
};
"#;
    assert_eq!(run(source), Value::Integer(20));
}

#[test]
fn array_length() {
    let source = r#"
let main = fn () {
    let arr = [1, 2, 3, 4, 5];
    return arr.length();
};
"#;
    assert_eq!(run(source), Value::Integer(5));
}

#[test]
fn nested_maps() {
    let source = r#"
let main = fn () {
    let outer = { inner: { value: 42 } };
    return outer.inner.value;
};
"#;
    assert_eq!(run(source), Value::Integer(42));
}

#[test]
fn nested_array_in_map() {
    let source = r#"
let main = fn () {
    let data = { items: [10, 20, 30] };
    let v = data.items[2];
    return v;
};
"#;
    assert_eq!(run(source), Value::Integer(30));
}

#[test]
fn spread_in_array() {
    let source = r#"
let main = fn () {
    let a = [1, 2];
    let b = [...a, 3, 4];
    return b;
};
"#;
    assert_eq!(
        run(source),
        Value::Array(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
        ])
    );
}

#[test]
fn spread_for_loop_in_array() {
    let source = r#"
let main = fn () {
    let arr = [...for (i in 0..3) { (i * 2) }];
    return arr;
};
"#;
    assert_eq!(
        run(source),
        Value::Array(vec![
            Value::Integer(0),
            Value::Integer(2),
            Value::Integer(4),
        ])
    );
}

#[test]
fn empty_array() {
    assert_eq!(eval("let arr = []; arr"), Value::Array(vec![]));
}

#[test]
fn array_of_strings() {
    assert_eq!(
        eval(r#"["a", "b", "c"]"#),
        Value::Array(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ])
    );
}

// ===========================================================================
// Widgets
// ===========================================================================

#[test]
fn flex_widget_returns_widget() {
    let source = r#"
let main = fn () {
    Flex {
        style: { width: "grow", height: "grow" },
        children: [],
    }
};
"#;
    let result = run(source);
    assert!(
        matches!(result, Value::Widget(_)),
        "expected Widget, got {:?}",
        result
    );
}

#[test]
fn text_widget_with_text_property() {
    let source = r#"
let main = fn () {
    Text {
        text: "Hello",
        style: { size: 16 },
    }
};
"#;
    let result = run(source);
    let Value::Widget(ref rw) = result else {
        panic!("expected Widget, got {:?}", result);
    };
    assert_eq!(rw.identifier.get(), "Text");
    assert_eq!(
        rw.properties.get("text"),
        Some(&Value::String("Hello".to_string()))
    );
}

#[test]
fn nested_widgets() {
    let source = r#"
let main = fn () {
    Flex {
        style: { width: "grow" },
        children: [
            Text { text: "child1", style: { size: 14 } },
            Text { text: "child2", style: { size: 14 } },
        ],
    }
};
"#;
    let result = run(source);
    let Value::Widget(ref rw) = result else {
        panic!("expected Widget, got {:?}", result);
    };
    assert_eq!(rw.identifier.get(), "Flex");
    let children = rw.properties.get("children").expect("should have children");
    let Value::Array(ref arr) = children else {
        panic!("children should be Array, got {:?}", children);
    };
    assert_eq!(arr.len(), 2);
}

// ===========================================================================
// Log statement
// ===========================================================================

#[test]
fn log_integer() {
    let source = "let main = fn () { log 42; };";
    run(source);
}

#[test]
fn log_string() {
    let source = r#"let main = fn () { log "hello"; };"#;
    run(source);
}

#[test]
fn log_expression() {
    let source = "let main = fn () { log 2 + 3; };";
    run(source);
}

// ===========================================================================
// Events
// ===========================================================================

#[test]
fn event_without_args() {
    let source = r#"let main = fn () { event("test_event"); };"#;
    run(source);
}

#[test]
fn event_with_arg() {
    let source = r#"let main = fn () { event("test_event", 42); };"#;
    run(source);
}

#[test]
fn event_handler_receives_args() {
    use std::sync::{Arc, Mutex};

    let received = Arc::new(Mutex::new(Vec::<i32>::new()));
    let received_clone = received.clone();

    let config = ogham::runtime::config::RuntimeConfig::new().with_event_handler(
        "my_event",
        move |args| {
            if let Some(Value::Integer(n)) = args.first() {
                received_clone.lock().unwrap().push(*n);
            }
            true
        },
    );

    let source = r#"
let main = fn () {
    event("my_event", 99);
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, Some(config)).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("execute");

    let r = received.lock().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0], 99);
}

// ===========================================================================
// Edge cases and errors
// ===========================================================================

#[test]
#[should_panic]
fn undefined_variable_errors() {
    eval("undefined_var");
}

#[test]
#[should_panic]
fn type_mismatch_comparison() {
    eval("1 == true");
}

#[test]
fn void_value() {
    let source = r#"
let main = fn () {
    let x = 1;
};
"#;
    assert_eq!(run(source), Value::Void);
}

#[test]
fn comparison_chain() {
    assert_eq!(eval("1 < 2 && 2 < 3"), Value::Boolean(true));
}

#[test]
fn complex_expression() {
    assert_eq!(eval("(2 + 3) * (4 - 1)"), Value::Integer(15));
}

#[test]
fn map_as_return_value() {
    let source = r#"
let main = fn () {
    { a: 1, b: 2 }
};
"#;
    let result = run(source);
    let Value::Map(m) = result else {
        panic!("expected Map, got {:?}", result);
    };
    assert_eq!(m.get("a"), Some(&Value::Integer(1)));
    assert_eq!(m.get("b"), Some(&Value::Integer(2)));
}

#[test]
fn array_computed_index() {
    let source = r#"
let main = fn () {
    let arr = [10, 20, 30];
    let i = 1;
    let v = arr[i + 1];
    return v;
};
"#;
    assert_eq!(run(source), Value::Integer(30));
}

#[test]
fn for_loop_with_array_length() {
    let source = r#"
let main = fn () {
    let items = [10, 20, 30];
    state sum = 0;
    for (i in 0..items.length()) {
        sum = sum + items[i];
    }
    sum
};
"#;
    assert_eq!(run(source), Value::Integer(60));
}

#[test]
fn deeply_nested_function() {
    let source = r#"
let a = fn (x: int) {
    return fn (y: int) {
        return fn (z: int): int { return x + y + z; };
    };
};
let main = fn () {
    a(1)(2)(3)
};
"#;
    assert_eq!(run(source), Value::Integer(6));
}

#[test]
fn match_returns_value() {
    let source = r#"
let main = fn () {
    let x = 1;
    let result = match x {
        1 => "one",
        _ => "other",
    };
    result
};
"#;
    assert_eq!(run(source), Value::String("one".to_string()));
}

#[test]
fn state_initialization_default() {
    let source = r#"
let main = fn () {
    state x = 42;
    x
};
"#;
    assert_eq!(run(source), Value::Integer(42));
}

#[test]
fn multiple_state_variables() {
    let source = r#"
let main = fn () {
    state a = 1;
    state b = 2;
    return a + b;
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    let first = runtime.execute_module(&module).expect("first");
    assert_eq!(first, Value::Integer(3));
}

#[test]
fn host_state_injection() {
    let mut initial = HashMap::new();
    initial.insert("greeting".to_string(), Value::String("Hello".to_string()));

    let config = ogham::runtime::config::RuntimeConfig::new().with_host_state(initial);

    let source = r#"
let main = fn () {
    greeting
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, Some(config)).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    let result = runtime.execute_module(&module).expect("execute");
    assert_eq!(result, Value::String("Hello".to_string()));
}

#[test]
fn function_as_value() {
    let source = r#"
let main = fn () {
    let f = fn (x: int): int { return x * 2; };
    f
};
"#;
    let result = run(source);
    assert!(
        matches!(result, Value::BytecodeClosure(_)),
        "expected closure, got {:?}",
        result
    );
}

#[test]
fn mixed_int_float_multiply() {
    assert_eq!(eval("3 * 2.5"), Value::Float(7.5));
}

#[test]
fn mixed_int_float_divide() {
    assert_eq!(eval("10 / 4.0"), Value::Float(2.5));
}

#[test]
fn double_negation() {
    assert_eq!(eval("--5"), Value::Integer(5));
}

#[test]
fn not_comparison_result() {
    assert_eq!(eval("!(1 == 2)"), Value::Boolean(true));
}

#[test]
fn chained_or_short_circuit() {
    assert_eq!(eval("true || false"), Value::Boolean(true));
}

#[test]
fn integer_zero_is_not_falsy_in_boolean_context() {
    assert_eq!(eval("0 == 0"), Value::Boolean(true));
}

#[test]
fn for_loop_expression_in_let() {
    let source = r#"
let main = fn () {
    let doubled = for (i in 0..3) { (i * 2) };
    return doubled.length();
};
"#;
    assert_eq!(run(source), Value::Integer(3));
}

#[test]
fn state_mutation_triggers_rerender_flag() {
    let source = r#"
let main = fn () {
    state x = 0;
    x = x + 1;
    x
};
"#;
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).expect("from_source");
    let module = runtime.get_module().expect("module").clone();
    runtime.execute_module(&module).expect("first");
    assert!(runtime.needs_rerender());
}

#[test]
fn map_with_mixed_value_types() {
    let source = r#"
let main = fn () {
    { name: "test", count: 42, active: true }
};
"#;
    let result = run(source);
    let Value::Map(m) = result else {
        panic!("expected Map, got {:?}", result);
    };
    assert_eq!(m.get("name"), Some(&Value::String("test".to_string())));
    assert_eq!(m.get("count"), Some(&Value::Integer(42)));
    assert_eq!(m.get("active"), Some(&Value::Boolean(true)));
}

#[test]
fn greater_than_false() {
    assert_eq!(eval("2 > 5"), Value::Boolean(false));
}

#[test]
fn less_than_false() {
    assert_eq!(eval("5 < 3"), Value::Boolean(false));
}
