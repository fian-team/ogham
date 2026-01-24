use std::sync::{Arc, Mutex};

pub mod parser;
pub mod runtime;
pub mod scanner;
pub mod skia;
pub mod tree;

pub struct Ogham {
    ui: tree::UI,
    watcher: Option<runtime::FileWatcher>,
    config: runtime::RuntimeConfig,
    runtime: Arc<Mutex<runtime::Runtime>>,
    path: Option<String>,
}

impl Ogham {
    /// Create an Ogham instance from a file path with file watching enabled
    pub fn watch(path: String, config: runtime::RuntimeConfig) -> Result<Self, runtime::RuntimeError> {
        let watcher = runtime::FileWatcher::new(path.clone())?;
        let runtime = Arc::new(Mutex::new(runtime::from_file(&path, Some(config.clone()))?));
        let ui = Self::create_ui_from_runtime(&runtime)?;
        Ok(Self {
            watcher: Some(watcher),
            runtime,
            config,
            ui,
            path: Some(path),
        })
    }

    /// Create an Ogham instance from source code (no file watching)
    pub fn from_source(source: &str, config: runtime::RuntimeConfig) -> Result<Self, runtime::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::from_source(source, Some(config.clone()))?));
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
    ) -> Result<tree::UI, runtime::RuntimeError> {
        let module = {
            let rt = runtime.lock().unwrap();
            rt.get_module().cloned().ok_or_else(|| {
                runtime::RuntimeError::VmError(runtime::VMError::InvalidOperation(
                    "No module stored in runtime".to_string(),
                ))
            })?
        };

        let widget_value = {
            let mut rt = runtime.lock().unwrap();
            rt.execute_module(&module)?
        };

        let widget_ref = tree::ast_bridge::widget_value_to_widget_ref(runtime, &widget_value)
            .map_err(|e| runtime::RuntimeError::BridgeError(e))?;
        Ok(tree::UI::new(widget_ref))
    }

    /// Check if the watched file has changed
    pub fn check_for_changes(&self) -> bool {
        self.watcher.as_ref().map(|w| w.check_for_changes()).unwrap_or(false)
    }

    /// Reload and recompile the current file
    pub fn reload(&mut self) -> Result<(), runtime::RuntimeError> {
        if let Some(path) = self.path.clone() {
            self.reload_file(&path)
        } else {
            Ok(()) // Nothing to reload if no file is being watched
        }
    }

    /// Load and watch a new file
    pub fn load_file(&mut self, path: String) -> Result<(), runtime::RuntimeError> {
        self.reload_file(&path)?;
        self.path = Some(path.clone());
        self.watcher = Some(runtime::FileWatcher::new(path)?);
        Ok(())
    }

    /// Reload a specific file (internal helper)
    fn reload_file(&mut self, path: &str) -> Result<(), runtime::RuntimeError> {
        let new_runtime = Arc::new(Mutex::new(runtime::from_file(path, Some(self.config.clone()))?));
        let new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        self.runtime = new_runtime;
        self.ui = new_ui;
        Ok(())
    }

    /// Recompile from source code
    pub fn recompile_from_source(&mut self, source: &str) -> Result<(), runtime::RuntimeError> {
        let new_runtime = Arc::new(Mutex::new(runtime::from_source(source, Some(self.config.clone()))?));
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
}
