use super::{expression::*, identifier::*};
use std::collections::HashMap;

#[derive(PartialEq, Clone, Debug)]
pub struct Widget {
    pub identifier: Identifier,
    pub properties: HashMap<String, Expression>,
}

impl Widget {
    pub fn new(identifier: Identifier) -> Widget {
        Widget {
            identifier,
            properties: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: Identifier, value: Expression) {
        self.properties.insert(key.get(), value);
    }
}
