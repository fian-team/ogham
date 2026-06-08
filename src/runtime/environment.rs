use std::collections::HashMap;

use crate::runtime::value::Value;

#[derive(Clone, Debug)]
pub struct Environment {
    variables: HashMap<String, Value>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.variables.get(name).cloned()
    }

    /// Returns only the variables defined in this environment.
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
}
