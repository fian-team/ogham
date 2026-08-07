use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

use crate::runtime::value::Value;
use crate::widget::builder::WidgetFactory;
use crate::widget::canvas_widget::{CanvasPainter, Painter};

/// A named font family with one or more file paths.
#[derive(Clone, Debug)]
pub struct FontEntry {
    pub family: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Default)]
pub struct RuntimeConfig {
    pub host_state: Option<std::collections::HashMap<String, Value>>,
    pub event_handlers:
        HashMap<String, Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>>,
    pub project_root: Option<PathBuf>,
    pub import_paths: HashMap<String, PathBuf>,
    /// In-memory sources for `import "..."` resolution, keyed by the import path
    /// string exactly as written (e.g. `"./chrome.ogh"`). Consulted BEFORE the
    /// filesystem, so a binary can embed its `.ogh` library via `include_str!`
    /// and run with no source-tree dependency.
    pub embedded_sources: HashMap<PathBuf, String>,
    pub fonts: Vec<FontEntry>,
    pub default_font: Option<String>,
    /// Custom widget factories keyed by lowercased type name. These are merged
    /// into the runtime's [`WidgetRegistry`] on creation, overriding built-in
    /// types if the names collide.
    pub custom_widgets: HashMap<String, WidgetFactory>,
    /// Host paint routines keyed by the exact name a `.ogh` file uses in
    /// `Canvas { painter: "name" }`. Copied onto the `Runtime` on creation;
    /// the `Canvas` builder resolves the name against this map at build time
    /// and rejects an unregistered one.
    ///
    /// Registering here rather than on a live `Runtime` is what makes
    /// painters survive hot reload: `Ogham::reload_file` rebuilds the
    /// runtime from this config, so a painter added post-hoc to the old
    /// runtime would vanish on the next file save.
    pub painters: HashMap<String, CanvasPainter>,
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
        F: Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
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

    /// Provide in-memory sources for `import "..."` resolution, keyed by the
    /// import path string exactly as written (e.g. `"./chrome.ogh"`). Consulted
    /// before the filesystem, so a binary can carry its `.ogh` library with
    /// `include_str!` and run with no source-tree dependency. The entry module
    /// itself is passed to [`crate::Ogham::from_source`]; its imports (and
    /// theirs, transitively) resolve from this map.
    pub fn with_embedded_sources(mut self, sources: HashMap<PathBuf, String>) -> Self {
        self.embedded_sources = sources;
        self
    }

    /// Register a named font family from one or more TTF/OTF file paths.
    /// Fonts registered here are automatically loaded when the `Ogham`
    /// instance is created via `watch()` or `from_source()`.
    pub fn with_font(
        mut self,
        family: impl Into<String>,
        paths: &[impl AsRef<std::path::Path>],
    ) -> Self {
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

    /// Register a custom widget type. The factory receives the widget
    /// registry (for building child widgets), the runtime, and the
    /// descriptor containing the widget's properties as declared in the
    /// `.ogh` source. Names are lowercased so lookups are case-insensitive.
    pub fn with_widget<S, F>(mut self, name: S, factory: F) -> Self
    where
        S: Into<String>,
        F: Fn(
                &crate::widget::builder::WidgetRegistry,
                &std::sync::Arc<std::sync::Mutex<crate::runtime::Runtime>>,
                &crate::runtime::descriptor::WidgetDescriptor,
            ) -> Result<crate::widget::WidgetRef, crate::widget::builder::BridgeError>
            + Send
            + Sync
            + 'static,
    {
        self.custom_widgets
            .insert(name.into().to_lowercase(), Arc::new(factory));
        self
    }

    /// Register a host paint routine that `.ogh` can seat in the layout as
    /// `Canvas { painter: "name", props: { … }, style: { … } }`.
    ///
    /// The closure receives a [`Painter`] whose canvas is already
    /// translated to the widget's laid-out origin and scaled by the
    /// backend's DPI factor — draw from `(0, 0)` to
    /// `(p.width(), p.height())` in logical pixels — plus the widget's
    /// `props:` map verbatim. Live host data comes from handles the
    /// closure captures here, the same way an event handler reads them.
    ///
    /// Names are matched **exactly** (unlike widget type names, which are
    /// lowercased): a painter name is an opaque string chosen by the host,
    /// not an identifier the language resolves. An unregistered name is a
    /// build-time error listing the registered ones, never a silent blank.
    ///
    /// ```rust,no_run
    /// # use ogham::runtime::config::RuntimeConfig;
    /// let config = RuntimeConfig::new().with_painter("wheel_dial", |p, _props| {
    ///     let mut paint = ogham::skia_safe::Paint::default();
    ///     paint.set_anti_alias(true);
    ///     p.canvas()
    ///         .draw_circle((p.width() / 2.0, p.height() / 2.0), p.width() / 2.0, &paint);
    /// });
    /// ```
    pub fn with_painter<S, F>(mut self, name: S, painter: F) -> Self
    where
        S: Into<String>,
        F: Fn(&mut Painter, &Value) + Send + Sync + 'static,
    {
        self.painters.insert(name.into(), Arc::new(painter));
        self
    }
}
