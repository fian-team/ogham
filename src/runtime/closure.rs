use crate::{parser::Function, runtime::Environment};

#[derive(Clone, Debug)]
pub struct Closure {
    pub function: Function,
    pub captured_env: Environment,
    pub captured_path: Vec<usize>,
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.function == other.function
    }
}
