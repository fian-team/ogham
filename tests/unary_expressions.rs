//! Integration tests for unary expressions (negation and logical not).

mod common;

use common::eval;
use ogham::runtime::value::Value;

#[test]
fn negate_integer() {
    assert_eq!(eval("-5"), Value::Integer(-5));
}

#[test]
fn negate_float() {
    assert_eq!(eval("-3.14"), Value::Float(-3.14));
}

#[test]
fn double_negate_integer() {
    assert_eq!(eval("--5"), Value::Integer(5));
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
fn binary_minus_with_unary_minus() {
    assert_eq!(eval("10 - -3"), Value::Integer(13));
}

#[test]
fn negate_variable() {
    assert_eq!(eval("let x = 42; -x"), Value::Integer(-42));
}

#[test]
fn not_variable() {
    assert_eq!(eval("let flag = true; !flag"), Value::Boolean(false));
}
