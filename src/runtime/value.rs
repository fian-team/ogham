use std::collections::HashMap;

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
