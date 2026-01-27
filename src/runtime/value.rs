use std::collections::HashMap;
use std::fmt;

use crate::runtime::{closure::Closure, widget::RuntimeWidget};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    String(String),
    Closure(Closure),
    Map(HashMap<String, Value>),
    Array(Vec<Value>),
    Widget(RuntimeWidget),
    Void,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "{}", s),
            Value::Closure(_) => write!(f, "<closure>"),
            Value::Map(_) => write!(f, "<map>"),
            Value::Array(_) => write!(f, "<array>"),
            Value::Widget(_) => write!(f, "<widget>"),
            Value::Void => write!(f, ""),
        }
    }
}
