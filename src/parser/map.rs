use super::{expression::*, identifier::*};
use std::collections::HashMap;

#[derive(PartialEq, Clone, Debug)]
pub struct Map {
    pub properties: HashMap<String, Expression>,
}

impl Map {
    pub fn new() -> Map {
        Map {
            properties: HashMap::new(),
        }
    }

    pub fn count(&self) -> usize {
        self.properties.len()
    }

    pub fn get(&self, identifier: &Identifier) -> Option<&Expression> {
        self.properties.get(&identifier.get())
    }

    pub fn set(&mut self, identifier: Identifier, value: Expression) {
        self.properties.insert(identifier.get(), value);
    }
}
