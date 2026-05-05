use std::collections::HashMap;

use crate::{parser::Identifier, runtime::value::Value};

#[derive(Clone, Debug, PartialEq)]
pub struct WidgetDescriptor {
    pub identifier: Identifier,
    pub properties: HashMap<String, Value>,
    /// Phase 2 lifecycle: the call-stack path captured at the
    /// moment the Widget opcode constructed this descriptor.
    /// Used by the widget builder to set
    /// [`Widget::owned_path_prefix`] on the materialized widget.
    /// Captured here (rather than at builder time) because
    /// `execute_module` returns with an empty call_stack — the
    /// path information is only valid during VM execution.
    pub owned_path: String,
}
