//! Conversion helpers for pushing host (Rust-side) state into the Ogham
//! runtime. The [`IntoHostValue`] trait removes the per-call
//! `Value::String(s.clone())` ceremony from binding sites: callers write
//! `rt.set_host_state("name", &fs.field)` and the trait picks the right
//! `Value` variant based on the field's type.
//!
//! Pair with [`Runtime::set_host_state`](crate::runtime::Runtime::set_host_state),
//! which only inserts (and triggers a rerender) when the new value differs
//! from the currently-stored one.
//!
//! See the trait impls below for the supported source types.

use std::collections::HashMap;

use crate::runtime::value::Value;
use crate::runtime::Runtime;

/// Converts a Rust value into the [`Value`] variant that host state
/// expects. Implemented for the common primitive and container types
/// touched by UI binding sites — extend as new shapes appear.
pub trait IntoHostValue {
    fn into_host_value(self) -> Value;
}

impl IntoHostValue for Value {
    fn into_host_value(self) -> Value {
        self
    }
}

impl IntoHostValue for bool {
    fn into_host_value(self) -> Value {
        Value::Boolean(self)
    }
}

impl IntoHostValue for i32 {
    fn into_host_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoHostValue for i64 {
    fn into_host_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoHostValue for u32 {
    fn into_host_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoHostValue for u64 {
    fn into_host_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoHostValue for usize {
    fn into_host_value(self) -> Value {
        Value::Integer(self as i32)
    }
}

impl IntoHostValue for f32 {
    fn into_host_value(self) -> Value {
        Value::Float(self as f64)
    }
}

impl IntoHostValue for f64 {
    fn into_host_value(self) -> Value {
        Value::Float(self)
    }
}

impl IntoHostValue for String {
    fn into_host_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoHostValue for &str {
    fn into_host_value(self) -> Value {
        Value::String(self.to_string())
    }
}

impl IntoHostValue for &String {
    fn into_host_value(self) -> Value {
        Value::String(self.clone())
    }
}

impl IntoHostValue for Vec<Value> {
    fn into_host_value(self) -> Value {
        Value::Array(self)
    }
}

impl IntoHostValue for HashMap<String, Value> {
    fn into_host_value(self) -> Value {
        Value::Map(self)
    }
}

/// `&HashMap<String, String>` → `Value::Map` of `Value::String`. Keeps
/// keybind-style maps from needing a manual rebuild at every binding site.
impl IntoHostValue for &HashMap<String, String> {
    fn into_host_value(self) -> Value {
        let map: HashMap<String, Value> = self
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        Value::Map(map)
    }
}

/// Common surface for "things you can push host state into" — implemented
/// for both [`Runtime`] (per-tick path: diffs, only triggers a rerender
/// when changed) and [`HashMap<String, Value>`] (initial-state path: feeds
/// `RuntimeConfig::with_host_state` so the very first module execution
/// has the keys it needs).
///
/// A single `push_state(&mut sink, &form_state)` function written against
/// this trait handles both call sites — no duplicated field list.
pub trait HostStateSink {
    fn set_value(&mut self, name: &str, value: Value);
}

impl HostStateSink for Runtime {
    fn set_value(&mut self, name: &str, value: Value) {
        self.set_host_state_value(name, value);
    }
}

impl HostStateSink for HashMap<String, Value> {
    fn set_value(&mut self, name: &str, value: Value) {
        self.insert(name.to_string(), value);
    }
}

/// Extension trait that lets callers write `sink.set("name", &fs.field)`
/// without ceremony — the [`IntoHostValue`] impl picks the right variant.
pub trait HostStateSinkExt: HostStateSink {
    fn set(&mut self, name: &str, value: impl IntoHostValue) {
        self.set_value(name, value.into_host_value());
    }
}

impl<T: HostStateSink + ?Sized> HostStateSinkExt for T {}
