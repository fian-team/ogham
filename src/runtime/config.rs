use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

use crate::runtime::value::Value;

/// A named font family with one or more file paths.
#[derive(Clone, Debug)]
pub struct FontEntry {
    pub family: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Default)]
pub struct RuntimeConfig {
    pub host_state: Option<std::collections::HashMap<String, Value>>,
    pub event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
    pub project_root: Option<PathBuf>,
    pub import_paths: HashMap<String, PathBuf>,
    pub fonts: Vec<FontEntry>,
    pub default_font: Option<String>,
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

    pub fn with_import_path(mut self, prefix: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.import_paths.insert(prefix.into(), path.into());
        self
    }

    /// Register a named font family from one or more TTF/OTF file paths.
    /// Fonts registered here are automatically loaded when the `Ogham`
    /// instance is created via `watch()` or `from_source()`.
    pub fn with_font(mut self, family: impl Into<String>, paths: &[impl AsRef<std::path::Path>]) -> Self {
        self.fonts.push(FontEntry {
            family: family.into(),
            paths: paths.iter().map(|p| p.as_ref().to_path_buf()).collect(),
        });
        self
    }

    /// Set the default font family applied to all text widgets that don't
    /// specify their own `font` style property.
    pub fn with_default_font(mut self, family: impl Into<String>) -> Self {
        self.default_font = Some(family.into());
        self
    }
}
