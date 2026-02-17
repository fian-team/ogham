//! # Ogham
//!
//! A UI language and framework inspired by DOM, CSS/Flexbox, and React.
//!
//! Ogham provides a custom language (`.ogh` files), a scanner/parser,
//! a bytecode compiler + VM runtime, a Flexbox-based widget tree, and
//! a Skia rendering backend.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use ogham::runtime::config::RuntimeConfig;
//!
//! let ogham = ogham::Ogham::from_source(
//!     r#"let main = fn () { 42 };"#,
//!     RuntimeConfig::default(),
//! ).expect("parse and execute");
//! ```

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod file_watcher;
pub mod parser;
pub mod runtime;
pub mod scanner;
pub mod skia;
pub mod tree;

/// Top-level Ogham instance that owns the runtime, widget tree, and
/// optional file watcher.
pub struct Ogham {
    ui: tree::UI,
    watcher: Option<file_watcher::FileWatcher>,
    config: runtime::config::RuntimeConfig,
    runtime: Arc<Mutex<runtime::Runtime>>,
    path: Option<String>,
}

impl Ogham {
    /// Create an Ogham instance from a file path with file watching enabled.
    /// Watches the main file and every imported file so that changes in any of them trigger a rerender.
    pub fn watch(
        path: String,
        config: runtime::config::RuntimeConfig,
    ) -> Result<Self, runtime::error::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::from_file(&path, Some(config.clone()))?));
        let ui = Self::create_ui_from_runtime(&runtime)?;
        let watch_paths = Self::paths_to_watch(&path, &runtime);
        let watcher = file_watcher::FileWatcher::new(watch_paths)?;
        Ok(Self {
            watcher: Some(watcher),
            runtime,
            config,
            ui,
            path: Some(path),
        })
    }

    /// Create an Ogham instance from source code (no file watching)
    pub fn from_source(
        source: &str,
        config: runtime::config::RuntimeConfig,
    ) -> Result<Self, runtime::error::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::from_source(
            source,
            Some(config.clone()),
        )?));
        let ui = Self::create_ui_from_runtime(&runtime)?;
        Ok(Self {
            watcher: None,
            runtime,
            config,
            ui,
            path: None,
        })
    }

    /// Helper function to create UI from a runtime
    fn create_ui_from_runtime(
        runtime: &Arc<Mutex<runtime::Runtime>>,
    ) -> Result<tree::UI, runtime::error::RuntimeError> {
        let module = {
            let rt = runtime.lock().expect("runtime lock poisoned");
            rt.get_module().cloned().ok_or_else(|| {
                runtime::error::RuntimeError::VmError(runtime::error::VMError::InvalidOperation(
                    "No module stored in runtime".to_string(),
                ))
            })?
        };

        let widget_value = {
            let mut rt = runtime.lock().expect("runtime lock poisoned");
            rt.execute_module(&module)?
        };

        let widget_ref = tree::ast_bridge::widget_value_to_widget_ref(runtime, &widget_value)
            .map_err(|e| runtime::error::RuntimeError::BridgeError(e))?;
        Ok(tree::UI::new(widget_ref))
    }

    /// Check if the watched file has changed
    pub fn check_for_changes(&self) -> bool {
        self.watcher
            .as_ref()
            .map(|w| w.check_for_changes())
            .unwrap_or(false)
    }

    /// Reload and recompile the current file
    pub fn reload(&mut self) -> Result<(), runtime::error::RuntimeError> {
        if let Some(path) = self.path.clone() {
            self.reload_file(&path)
        } else {
            Ok(()) // Nothing to reload if no file is being watched
        }
    }

    /// Load and watch a new file (and all its imports)
    pub fn load_file(&mut self, path: String) -> Result<(), runtime::error::RuntimeError> {
        self.reload_file(&path)?;
        self.path = Some(path.clone());
        let watch_paths = Self::paths_to_watch(&path, &self.runtime);
        self.watcher = Some(file_watcher::FileWatcher::new(watch_paths)?);
        Ok(())
    }

    /// Build the list of paths to watch: main file plus every imported module.
    fn paths_to_watch(main_path: &str, runtime: &Arc<Mutex<runtime::Runtime>>) -> Vec<PathBuf> {
        let mut paths = vec![PathBuf::from(main_path)];
        paths.extend(runtime.lock().expect("runtime lock poisoned").get_imported_paths());
        paths
    }

    /// Reload a specific file (internal helper)
    fn reload_file(&mut self, path: &str) -> Result<(), runtime::error::RuntimeError> {
        let new_runtime = Arc::new(Mutex::new(runtime::from_file(
            path,
            Some(self.config.clone()),
        )?));
        let new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        self.runtime = new_runtime;
        self.ui = new_ui;
        Ok(())
    }

    /// Recompile from source code
    pub fn recompile_from_source(
        &mut self,
        source: &str,
    ) -> Result<(), runtime::error::RuntimeError> {
        let new_runtime = Arc::new(Mutex::new(runtime::from_source(
            source,
            Some(self.config.clone()),
        )?));
        let new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        self.runtime = new_runtime;
        self.ui = new_ui;
        Ok(())
    }

    /// Get a reference to the UI
    pub fn get_ui(&self) -> &tree::UI {
        &self.ui
    }

    /// Get a mutable reference to the UI
    pub fn get_ui_mut(&mut self) -> &mut tree::UI {
        &mut self.ui
    }

    /// Get a reference to the runtime
    pub fn get_runtime(&self) -> &Arc<Mutex<runtime::Runtime>> {
        &self.runtime
    }

    /// Get the current file path being watched
    pub fn get_path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// If the runtime has flagged a rerender, re-execute the module,
    /// bridge the resulting widget values into the widget tree, and
    /// reconcile. Returns `true` if a rerender was performed.
    pub fn update(&mut self) -> Result<bool, runtime::error::RuntimeError> {
        let widget_value = {
            let mut rt = self.runtime.lock().expect("runtime lock poisoned");
            if !rt.needs_rerender() {
                return Ok(false);
            }
            rt.rerender()?
        };

        let widget_ref =
            tree::ast_bridge::widget_value_to_widget_ref(&self.runtime, &widget_value)?;

        self.ui.reconcile(widget_ref);
        Ok(true)
    }
}
