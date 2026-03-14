use std::collections::HashMap;

use crate::{parser::Identifier, runtime::value::Value};

#[derive(Clone, Debug, PartialEq)]
pub struct WidgetDescriptor {
    pub identifier: Identifier,
    pub properties: HashMap<String, Value>,
}
