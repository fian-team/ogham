use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

use crate::runtime::schema::OghamMsg;
use crate::runtime::value::Value;
use crate::widget::builder::WidgetFactory;

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
    pub fonts: Vec<FontEntry>,
    pub default_font: Option<String>,
    /// Custom widget factories keyed by lowercased type name. These are merged
    /// into the runtime's [`WidgetRegistry`] on creation, overriding built-in
    /// types if the names collide.
    pub custom_widgets: HashMap<String, WidgetFactory>,
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

    /// Wire typed event handlers for `M` onto this config and return the
    /// receiver the host drains. One handler is registered per event declared
    /// by `M::ogham_events()`; each decodes its args into the matching `M`
    /// variant and pushes it onto the channel.
    ///
    /// This is the *decoupled* half of [`crate::TypedOgham`]: use it when the
    /// host owns the `Ogham` instance elsewhere (e.g. a view-tree
    /// `Application`) and only needs the `Receiver<M>` on the side. Drain it
    /// after the instance ticks (poll-after-tick).
    pub fn with_typed_events<M: OghamMsg>(mut self) -> (Self, Receiver<M>) {
        let (tx, rx) = channel::<M>();
        // Shared across the per-event handlers so the channel stays open even
        // as individual handlers are replaced; `Mutex` satisfies `Sync`.
        let tx = Arc::new(Mutex::new(tx));
        let event_names: Vec<String> = M::ogham_events().keys().cloned().collect();
        for name in event_names {
            let tx_for_handler = tx.clone();
            let event_name = name.clone();
            self = self.with_event_handler(name, move |args: &[Value]| {
                if let Some(msg) = M::try_from_ogham_event(&event_name, args) {
                    if let Ok(sender) = tx_for_handler.lock() {
                        let _ = sender.send(msg);
                    }
                }
                Ok(Value::Void)
            });
        }
        (self, rx)
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
}
