use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::runtime::{closure::Closure, opcode::VMClosure, widget::RuntimeWidget};

#[derive(Clone, Debug)]
pub enum Value {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    String(String),
    Closure(Closure),
    /// A bytecode closure produced by the bytecode compiler / VM.
    BytecodeClosure(Rc<VMClosure>),
    Map(HashMap<String, Value>),
    Array(Vec<Value>),
    Widget(RuntimeWidget),
    Void,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Closure(a), Value::Closure(b)) => a == b,
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
            Value::Closure(_) => write!(f, "<closure>"),
            Value::BytecodeClosure(_) => write!(f, "<closure>"),
            Value::Map(_) => write!(f, "<map>"),
            Value::Array(_) => write!(f, "<array>"),
            Value::Widget(_) => write!(f, "<widget>"),
            Value::Void => write!(f, ""),
        }
    }
}
