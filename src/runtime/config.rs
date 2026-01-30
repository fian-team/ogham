use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

use crate::runtime::value::Value;

#[derive(Clone, Default)]
pub struct RuntimeConfig {
    pub host_state: Option<std::collections::HashMap<String, Value>>,
    pub event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
    pub project_root: Option<PathBuf>,
}

impl RuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_host_state(mut self, state: std::collections::HashMap<String, Value>) -> Self {
        self.host_state = Some(state);
        self
    }

    pub fn with_event_handler<S, F>(mut self, name: S, handler: F) -> Self
    where
        S: Into<String>,
        F: Fn(&[Value]) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
        self
    }

    pub fn with_project_root(mut self, path: PathBuf) -> Self {
        self.project_root = Some(path);
        self
    }
}
