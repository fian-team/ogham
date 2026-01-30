use std::collections::HashMap;

use crate::runtime::value::Value;

#[derive(Clone, Debug)]
pub struct Environment {
    variables: HashMap<String, Value>,
    parent: Option<Box<Environment>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            parent: None,
        }
    }

    pub fn new_with_parent(parent: Environment) -> Self {
        Self {
            variables: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.variables.get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
    }

    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        if self.variables.contains_key(name) {
            self.variables.insert(name.to_string(), value);
            true
        } else if let Some(parent) = &mut self.parent {
            parent.assign(name, value)
        } else {
            false
        }
    }

    /// Returns only the variables defined in this environment (not parent).
    pub fn top_level_variables(&self) -> &HashMap<String, Value> {
        &self.variables
    }

    /// Copies names from `other` into self. If `names` is Some, only those names are copied
    /// (caller must ensure they exist in other). If `names` is None, all top-level variables
    /// from `other` are copied. If `check_conflict` is true, returns Err(conflicting_name)
    /// when a name already exists in self.
    pub fn copy_from(
        &mut self,
        other: &Environment,
        names: Option<&[String]>,
        check_conflict: bool,
    ) -> Result<(), String> {
        let to_copy: Vec<(String, Value)> = match names {
            Some(ns) => ns
                .iter()
                .filter_map(|n| other.get(n).map(|v| (n.clone(), v)))
                .collect(),
            None => other
                .top_level_variables()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        for (name, value) in to_copy {
            if check_conflict && self.variables.contains_key(&name) {
                return Err(name);
            }
            self.variables.insert(name, value);
        }
        Ok(())
    }

    /// Returns true if this environment (top-level only) defines the given name.
    pub fn defines(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }
}
