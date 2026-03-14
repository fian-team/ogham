//! Runtime value representation for all Ogham types.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::runtime::{descriptor::WidgetDescriptor, opcode::VMClosure};

/// A dynamically-typed value produced and consumed by the Ogham runtime.
#[derive(Clone, Debug)]
pub enum Value {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    String(String),
    /// A bytecode closure produced by the bytecode compiler / VM.
    BytecodeClosure(Rc<VMClosure>),
    Map(HashMap<String, Value>),
    Array(Vec<Value>),
    Widget(WidgetDescriptor),
    Void,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::BytecodeClosure(a), Value::BytecodeClosure(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Widget(a), Value::Widget(b)) => a == b,
            (Value::Void, Value::Void) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "{}", s),
            Value::BytecodeClosure(_) => write!(f, "<closure>"),
            Value::Map(_) => write!(f, "<map>"),
            Value::Array(_) => write!(f, "<array>"),
            Value::Widget(_) => write!(f, "<widget>"),
            Value::Void => write!(f, ""),
        }
    }
}

/// Trait for converting Rust values into Ogham `Value`s.
pub trait IntoOghamValue {
    fn into_ogham_value(self) -> Value;
}

impl IntoOghamValue for i32 {
    fn into_ogham_value(self) -> Value { Value::Integer(self) }
}

impl IntoOghamValue for u32 {
    fn into_ogham_value(self) -> Value { Value::Integer(self as i32) }
}

impl IntoOghamValue for u64 {
    fn into_ogham_value(self) -> Value { Value::Integer(self as i32) }
}

impl IntoOghamValue for f32 {
    fn into_ogham_value(self) -> Value { Value::Float(self as f64) }
}

impl IntoOghamValue for f64 {
    fn into_ogham_value(self) -> Value { Value::Float(self) }
}

impl IntoOghamValue for bool {
    fn into_ogham_value(self) -> Value { Value::Boolean(self) }
}

impl IntoOghamValue for String {
    fn into_ogham_value(self) -> Value { Value::String(self) }
}

impl IntoOghamValue for &str {
    fn into_ogham_value(self) -> Value { Value::String(self.to_string()) }
}

impl<T: IntoOghamValue> IntoOghamValue for Option<T> {
    fn into_ogham_value(self) -> Value {
        match self {
            Some(v) => v.into_ogham_value(),
            None => Value::Void,
        }
    }
}

impl<T: IntoOghamValue> IntoOghamValue for Vec<T> {
    fn into_ogham_value(self) -> Value {
        Value::Array(self.into_iter().map(|v| v.into_ogham_value()).collect())
    }
}

impl IntoOghamValue for Value {
    fn into_ogham_value(self) -> Value { self }
}
