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
    /// A map, shared rather than copied. `Value` is `Clone` at every read
    /// the VM makes — `GetHostState`, `GetLocal`, `GetProperty` — and a
    /// deep copy made every read of `rows[i]` cost the whole of `rows`,
    /// quadratic in a list's length (measured 2026-09-08: an 800-row
    /// sidebar rerendered in 194 ms, 9 ms without the copies). The VM has
    /// no opcode that mutates a container after it is built, so sharing
    /// needs no copy-on-write: a builder fills a `HashMap` and wraps it.
    Map(Rc<HashMap<String, Value>>),
    /// An array, shared for `Map`'s reason.
    Array(Rc<Vec<Value>>),
    Widget(WidgetDescriptor),
    Void,
    /// Phase 2.5 M2: an opaque widget identity, produced by
    /// the `focused_widget()` built-in and consumed by
    /// `focus(ref)`. The inner u64 is a per-UI counter
    /// allocated by `WidgetTree`. Identifies a widget
    /// instance within a single UI tree.
    WidgetRef(u64),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::BytecodeClosure(a), Value::BytecodeClosure(b)) => a == b,
            // Pointer equality first: a host that republishes the value it
            // published last frame is answered without a walk.
            (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b) || a == b,
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b) || a == b,
            (Value::Widget(a), Value::Widget(b)) => a == b,
            (Value::Void, Value::Void) => true,
            (Value::WidgetRef(a), Value::WidgetRef(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    /// An array value over `items`.
    pub fn array(items: impl Into<Vec<Value>>) -> Value {
        Value::Array(Rc::new(items.into()))
    }

    /// A map value over `fields`.
    pub fn map(fields: impl Into<HashMap<String, Value>>) -> Value {
        Value::Map(Rc::new(fields.into()))
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Value {
        Value::array(items)
    }
}

impl From<HashMap<String, Value>> for Value {
    fn from(fields: HashMap<String, Value>) -> Value {
        Value::map(fields)
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
            Value::WidgetRef(id) => write!(f, "<widget#{}>", id),
        }
    }
}

/// Trait for converting Rust values into Ogham `Value`s.
pub trait IntoOghamValue {
    fn into_ogham_value(self) -> Value;
}

impl IntoOghamValue for i32 {
    fn into_ogham_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoOghamValue for u32 {
    fn into_ogham_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoOghamValue for u64 {
    fn into_ogham_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoOghamValue for f32 {
    fn into_ogham_value(self) -> Value {
        Value::Float(self as f64)
    }
}

impl IntoOghamValue for f64 {
    fn into_ogham_value(self) -> Value {
        Value::Float(self)
    }
}

impl IntoOghamValue for bool {
    fn into_ogham_value(self) -> Value {
        Value::Boolean(self)
    }
}

impl IntoOghamValue for String {
    fn into_ogham_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoOghamValue for &str {
    fn into_ogham_value(self) -> Value {
        Value::String(self.to_string())
    }
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
        Value::array(
            self.into_iter()
                .map(|v| v.into_ogham_value())
                .collect::<Vec<_>>(),
        )
    }
}

impl IntoOghamValue for Value {
    fn into_ogham_value(self) -> Value {
        self
    }
}
