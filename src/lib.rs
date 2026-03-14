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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use skia_safe::textlayout::FontCollection;
use skia_safe::{FontMgr, Typeface};

mod file_watcher;
pub mod parser;
pub mod runtime;
pub mod scanner;
pub mod skia;
pub mod widget;
mod macros;

/// Top-level Ogham instance that owns the runtime, widget tree, and
/// optional file watcher.
pub struct Ogham {
    ui: widget::UI,
    watcher: Option<file_watcher::FileWatcher>,
    config: runtime::config::RuntimeConfig,
    runtime: Arc<Mutex<runtime::Runtime>>,
    path: Option<String>,
    font_collection: Option<FontCollection>,
    /// Raw typeface data keyed by family name, kept so we can rebuild the
    /// collection after registering additional fonts.
    registered_typefaces: Vec<(String, Typeface)>,
    /// Default font family applied to all text widgets that don't set
    /// their own `font` style property.
    default_font: Option<String>,
}

impl Ogham {
    /// Create an Ogham instance from a file path with file watching enabled.
    /// Watches the main file and every imported file so that changes in any of them trigger a rerender.
    pub fn watch(
        path: String,
        config: runtime::config::RuntimeConfig,
    ) -> Result<Self, runtime::error::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::Runtime::from_file(&path, Some(config.clone()))?));
        let ui = Self::create_ui_from_runtime(&runtime)?;
        let watch_paths = Self::paths_to_watch(&path, &runtime);
        let watcher = file_watcher::FileWatcher::new(watch_paths)?;
        let mut instance = Self {
            watcher: Some(watcher),
            runtime,
            config: config.clone(),
            ui,
            path: Some(path),
            font_collection: None,
            registered_typefaces: Vec::new(),
            default_font: None,
        };
        instance.apply_config_fonts(&config);
        Ok(instance)
    }

    /// Create an Ogham instance from source code (no file watching)
    pub fn from_source(
        source: &str,
        config: runtime::config::RuntimeConfig,
    ) -> Result<Self, runtime::error::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::Runtime::from_source(
            source,
            Some(config.clone()),
        )?));
        let ui = Self::create_ui_from_runtime(&runtime)?;
        let mut instance = Self {
            watcher: None,
            runtime,
            config: config.clone(),
            ui,
            path: None,
            font_collection: None,
            registered_typefaces: Vec::new(),
            default_font: None,
        };
        instance.apply_config_fonts(&config);
        Ok(instance)
    }

    /// Register fonts and set the default font family from the config.
    fn apply_config_fonts(&mut self, config: &runtime::config::RuntimeConfig) {
        for entry in &config.fonts {
            let paths: Vec<&Path> = entry.paths.iter().map(|p| p.as_path()).collect();
            self.register_font(&entry.family, &paths);
        }
        if let Some(ref name) = config.default_font {
            self.set_default_font(name);
        }
    }

    /// Helper function to create UI from a runtime
    fn create_ui_from_runtime(
        runtime: &Arc<Mutex<runtime::Runtime>>,
    ) -> Result<widget::UI, runtime::error::RuntimeError> {
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

        let widget_ref = widget::builder::widget_value_to_widget_ref(runtime, &widget_value)
            .map_err(|e| runtime::error::RuntimeError::BridgeError(e))?;
        Ok(widget::UI::new(widget_ref))
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
        let new_runtime = Arc::new(Mutex::new(runtime::Runtime::from_file(
            path,
            Some(self.config.clone()),
        )?));
        let mut new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        if let Some(ref fc) = self.font_collection {
            new_ui.set_font_collection(fc.clone());
        }
        if let Some(ref name) = self.default_font {
            new_ui.set_default_font(name.clone());
        }
        self.runtime = new_runtime;
        self.ui = new_ui;
        Ok(())
    }

    /// Recompile from source code
    pub fn recompile_from_source(
        &mut self,
        source: &str,
    ) -> Result<(), runtime::error::RuntimeError> {
        let new_runtime = Arc::new(Mutex::new(runtime::Runtime::from_source(
            source,
            Some(self.config.clone()),
        )?));
        let mut new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        if let Some(ref fc) = self.font_collection {
            new_ui.set_font_collection(fc.clone());
        }
        if let Some(ref name) = self.default_font {
            new_ui.set_default_font(name.clone());
        }
        self.runtime = new_runtime;
        self.ui = new_ui;
        Ok(())
    }

    /// Get a reference to the UI
    pub fn get_ui(&self) -> &widget::UI {
        &self.ui
    }

    /// Get a mutable reference to the UI
    pub fn get_ui_mut(&mut self) -> &mut widget::UI {
        &mut self.ui
    }

    /// Get a reference to the runtime
    pub fn get_runtime(&self) -> &Arc<Mutex<runtime::Runtime>> {
        &self.runtime
    }

    /// Run a closure with exclusive access to the runtime, internalizing
    /// the mutex lock (with poisoned-mutex recovery).
    pub fn with_runtime_mut<R>(&self, f: impl FnOnce(&mut runtime::Runtime) -> R) -> R {
        let mut guard = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    /// Get the current file path being watched
    pub fn get_path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Set a default font family that will be used by all text widgets
    /// that don't explicitly specify a `font` in their style.
    pub fn set_default_font(&mut self, name: &str) {
        self.default_font = Some(name.to_string());
        self.ui.set_default_font(name.to_string());
    }

    /// Register a named font family from one or more TTF/OTF files.
    ///
    /// Each file is loaded as a `Typeface` and registered under the given
    /// `family` name. Skia will select the appropriate weight automatically
    /// based on the metadata embedded in each font file.
    ///
    /// Can be called multiple times with the same family to add more weights,
    /// or with different families to register additional fonts.
    ///
    /// # Panics
    ///
    /// Panics if a file cannot be read or does not contain a valid font.
    pub fn register_font(&mut self, family: &str, paths: &[impl AsRef<Path>]) {
        let font_mgr = FontMgr::new();
        for path in paths {
            let bytes = std::fs::read(path.as_ref())
                .unwrap_or_else(|e| panic!("failed to read font file {:?}: {}", path.as_ref(), e));
            let typeface = font_mgr
                .new_from_data(&bytes, None)
                .unwrap_or_else(|| panic!("invalid font file: {:?}", path.as_ref()));
            self.registered_typefaces
                .push((family.to_string(), typeface));
        }
        self.rebuild_font_collection();
    }

    fn rebuild_font_collection(&mut self) {
        use skia_safe::textlayout::TypefaceFontProvider;

        let mut provider = TypefaceFontProvider::new();
        for (family, typeface) in &self.registered_typefaces {
            provider.register_typeface(typeface.clone(), Some(family.as_str()));
        }

        let mut fc = FontCollection::new();
        fc.set_asset_font_manager(Some(provider.into()));
        fc.set_default_font_manager(FontMgr::new(), None);

        self.ui.set_font_collection(fc.clone());
        self.font_collection = Some(fc);
    }

    /// Perform a complete frame update: check for file changes, reload if
    /// needed, run the `inject` callback to push host state into the
    /// runtime, then reconcile the widget tree if a rerender is pending.
    ///
    /// Returns `true` if a rerender was performed.
    pub fn tick(
        &mut self,
        inject: impl FnOnce(&mut runtime::Runtime),
    ) -> Result<bool, runtime::error::RuntimeError> {
        if self.check_for_changes() {
            self.reload()?;
        }
        self.with_runtime_mut(inject);
        self.update()
    }

    /// Read an Ogham state variable by name from the runtime.
    pub fn get_state(&self, name: &str) -> Option<runtime::value::Value> {
        self.with_runtime_mut(|rt| rt.get_state(name))
    }

    /// Update the screen dimensions exposed as built-in variables
    /// (`screen_width`, `screen_height`). Call this before `tick()` or
    /// whenever the window size changes.
    pub fn set_screen_size(&self, width: f32, height: f32) {
        self.with_runtime_mut(|rt| rt.set_screen_size(width, height));
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
            widget::builder::widget_value_to_widget_ref(&self.runtime, &widget_value)?;

        self.ui.reconcile(widget_ref);
        Ok(true)
    }
}
