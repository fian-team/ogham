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

/// The exact `skia_safe` Ogham links against, re-exported for hosts that
/// paint through the [`Canvas`](widget::canvas_widget) hatch.
///
/// [`Painter::canvas`](widget::canvas_widget::Painter::canvas) hands back a
/// `skia_safe::Canvas`, so a painter has to name Skia types. Depending on
/// `skia-safe` separately risks two *different* copies of the crate in one
/// binary — the drawing calls then don't typecheck, with an error message
/// that blames the wrong thing. Go through this re-export instead.
///
/// This is the only backend-native surface Ogham exposes; see
/// [`INTENT.md`](../docs/internal/INTENT.md) §6.
pub use skia_safe;

pub mod contract;
mod file_watcher;
mod macros;
pub mod parser;
pub mod route;
pub mod runtime;
pub mod scanner;
pub mod skia;
pub mod widget;

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
    /// Base directory for `Image { path }` lookups, kept so hot reloads
    /// re-point the fresh UI's image cache at it (like fonts).
    image_root: Option<PathBuf>,
}

/// What one [`Ogham::frame`] call did.
///
/// `rerendered` is the old bare-bool return. `reloaded` exists because a
/// hot reload used to happen silently inside `frame`, and the two answers
/// a wrapping host caches — its projection diff and its document-vs-table
/// validation ([`route::Chrome`]) — both die with the runtime the reload
/// replaced. A host that is not told a swap happened cannot re-ask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameReport {
    /// A watched file changed and the runtime was rebuilt from disk.
    /// State-preserving: live host state is carried into the new runtime.
    pub reloaded: bool,
    /// The module re-executed this frame.
    pub rerendered: bool,
}

/// Why a gated reload did not take (`APPLICATION.md` §4.1).
///
/// Either way the running instance stands: the front door stays open, the
/// file stays watched, and fixing the edit heals the session without a
/// restart. The two arms differ only in what to tell the author.
#[derive(Debug)]
pub enum ReloadRefused {
    /// The edited document does not compile, or its fresh widget tree would
    /// not build.
    Broken(runtime::error::RuntimeError),
    /// It compiles, and the check refused it — a selection naming a field
    /// nothing provides, a raise nothing accepts. The sentence names the
    /// offender, because the case being designed for is a modder's stale
    /// expectation.
    Refused(String),
}

impl std::fmt::Display for ReloadRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReloadRefused::Broken(e) => write!(f, "{e:?}"),
            ReloadRefused::Refused(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ReloadRefused {}

impl Ogham {
    /// Create an Ogham instance from a file path with file watching enabled.
    /// Watches the main file and every imported file so that changes in any of them trigger a rerender.
    pub fn watch(
        path: String,
        config: runtime::config::RuntimeConfig,
    ) -> Result<Self, runtime::error::RuntimeError> {
        let runtime = Arc::new(Mutex::new(runtime::Runtime::from_file(
            &path,
            Some(config.clone()),
        )?));
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
            image_root: None,
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
            image_root: None,
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
        let (module, registry) = {
            let rt = runtime.lock().expect("runtime lock poisoned");
            let module = rt.get_module().cloned().ok_or_else(|| {
                runtime::error::RuntimeError::VmError(runtime::error::VMError::InvalidOperation(
                    "No module stored in runtime".to_string(),
                ))
            })?;
            (module, rt.widget_registry.clone())
        };

        let widget_value = {
            let mut rt = runtime.lock().expect("runtime lock poisoned");
            rt.execute_module(&module)?
        };

        let widget_ref =
            widget::builder::widget_value_to_widget_ref(&registry, runtime, &widget_value)
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

    /// Reload the watched file, and take the result only if `accept` says
    /// the fresh document still holds its contract.
    ///
    /// `APPLICATION.md` §4.1: validation runs at document load **and at
    /// every hot reload**, and a hot-reload refusal rejects the new
    /// document *without tearing down the running instance*. That last
    /// clause is what this method is for. `accept` is called on the
    /// candidate's own module schema after it compiles and before anything
    /// of the running instance has been touched — so a refusal costs the
    /// candidate and nothing else: the running runtime, its widget tree,
    /// its focus stack and its live host state all stand, and the file
    /// stays watched, so fixing the edit heals the session with no restart.
    ///
    /// A document that compiles but does not resolve to a schema is passed
    /// through unchecked, for the reason
    /// [`module_schema`](Self::module_schema) returns `None` there: it has
    /// a real error waiting with real diagnostics, and a second complaint
    /// on top would bury it.
    pub fn reload_if(
        &mut self,
        accept: impl FnOnce(&runtime::schema::ModuleSchema) -> Result<(), String>,
    ) -> Result<(), ReloadRefused> {
        match self.path.clone() {
            Some(path) => self.reload_file_if(&path, accept),
            None => Ok(()),
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
        paths.extend(
            runtime
                .lock()
                .expect("runtime lock poisoned")
                .get_imported_paths(),
        );
        paths
    }

    /// Reload a specific file (internal helper)
    fn reload_file(&mut self, path: &str) -> Result<(), runtime::error::RuntimeError> {
        self.reload_file_if(path, |_| Ok(()))
            .map_err(|refused| match refused {
                ReloadRefused::Broken(e) => e,
                ReloadRefused::Refused(_) => {
                    unreachable!("an ungated reload accepts everything that compiles")
                }
            })
    }

    /// [`reload_file`](Self::reload_file) with the gate
    /// [`reload_if`](Self::reload_if) supplies.
    ///
    /// The order is the whole point: the candidate is built and offered to
    /// `accept` **before** the running instance is touched at all. Every
    /// early return above the swap leaves this `Ogham` exactly as it was.
    fn reload_file_if(
        &mut self,
        path: &str,
        accept: impl FnOnce(&runtime::schema::ModuleSchema) -> Result<(), String>,
    ) -> Result<(), ReloadRefused> {
        let new_runtime = Arc::new(Mutex::new(
            runtime::Runtime::from_file(path, Some(self.config.clone()))
                .map_err(ReloadRefused::Broken)?,
        ));
        // Resolved against the candidate's *own* import graph: a hot edit
        // may add, drop or redirect an import, and a gate that read the
        // running document's imports would check the wrong document. A
        // candidate that imports a record and does not resolve without it
        // would otherwise skip the gate entirely (WP-3.1).
        let candidate = {
            let rt = new_runtime.lock().expect("runtime lock poisoned");
            rt.module_schema()
        };
        if let Some(schema) = candidate {
            accept(&schema).map_err(ReloadRefused::Refused)?;
        }

        // Phase 2.5 M3: clear the OLD UI's lifecycle state
        // (focus stack + portal_layers + focused) before
        // dropping it. Prevents stale focus restoration
        // pointing at widgets that no longer exist in the
        // reloaded tree. Below the gate, because a refused
        // candidate must leave the running tree's focus alone.
        self.ui.clear_lifecycle_state();
        self.carry_host_state_into(&new_runtime);
        let mut new_ui =
            Self::create_ui_from_runtime(&new_runtime).map_err(ReloadRefused::Broken)?;
        if let Some(ref fc) = self.font_collection {
            new_ui.set_font_collection(fc.clone());
        }
        if let Some(ref name) = self.default_font {
            new_ui.set_default_font(name.clone());
        }
        if let Some(ref root) = self.image_root {
            new_ui.image_cache.set_root(root.clone());
        }
        self.runtime = new_runtime;
        self.ui = new_ui;
        self.rewatch(path);
        Ok(())
    }

    /// Re-derive the watch set from the runtime that just took over.
    ///
    /// A hot edit can add an import, drop one, or point one somewhere
    /// else, and the watch set is a *reading* of the import graph rather
    /// than a fact about the mount. Without this, a shared module added to
    /// a document by an edit is invisible until the process restarts —
    /// which is the whole of WP-3.1's promise ("an edit to a shared module
    /// reloads every mounting document") failing on the second save.
    ///
    /// Only when this instance was already watching: an unwatched
    /// instance whose host drives its own reloads must not silently
    /// acquire a watcher. A watcher that cannot be built (a file removed
    /// underneath us) leaves the old one in place rather than leaving the
    /// instance unwatched.
    fn rewatch(&mut self, path: &str) {
        if self.watcher.is_none() {
            return;
        }
        let paths = Self::paths_to_watch(path, &self.runtime);
        match file_watcher::FileWatcher::new(paths) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(e) => eprintln!("ogham: the watch set could not be rebuilt: {e:?}"),
        }
    }

    /// Carry the live host-state map from the current runtime into a
    /// replacement one. Hot reload rebuilds the runtime from the config's
    /// *initial* host state; without this, a reload snaps the tree back to
    /// placeholders until the host next injects. Must run before the new
    /// runtime's first module execution so even the first reloaded frame
    /// renders live data.
    fn carry_host_state_into(&self, new_runtime: &Arc<Mutex<runtime::Runtime>>) {
        let live = self
            .runtime
            .lock()
            .expect("runtime lock poisoned")
            .host_state_snapshot();
        new_runtime
            .lock()
            .expect("runtime lock poisoned")
            .inject_host_state_batch(live);
    }

    /// Recompile from source code
    pub fn recompile_from_source(
        &mut self,
        source: &str,
    ) -> Result<(), runtime::error::RuntimeError> {
        // Phase 2.5 M3: same hot-reload reset as reload_file.
        self.ui.clear_lifecycle_state();

        let new_runtime = Arc::new(Mutex::new(runtime::Runtime::from_source(
            source,
            Some(self.config.clone()),
        )?));
        self.carry_host_state_into(&new_runtime);
        let mut new_ui = Self::create_ui_from_runtime(&new_runtime)?;
        if let Some(ref fc) = self.font_collection {
            new_ui.set_font_collection(fc.clone());
        }
        if let Some(ref name) = self.default_font {
            new_ui.set_default_font(name.clone());
        }
        if let Some(ref root) = self.image_root {
            new_ui.image_cache.set_root(root.clone());
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

    /// Phase 2 M4: returns `true` if any active Portal in the
    /// most recent draw has `focus_trap: true`. Hosts use this
    /// to derive their own input-gating booleans (e.g. UL
    /// replaces its manual `overlay_active: bool` with one
    /// derivation: `let overlay_active =
    /// ogham.has_input_blocking_portal();`).
    pub fn has_input_blocking_portal(&self) -> bool {
        self.ui.has_input_blocking_portal()
    }

    /// Phase 2.5 M1: returns `true` if any active Portal or
    /// the focused widget declares `CursorPreference::Free`.
    /// Hosts compose this with their own cursor-lock demand.
    /// Replaces UL's manual "is anything focused / overlay
    /// open?" plumbing in `update.rs:1561+`.
    pub fn wants_cursor_free(&self) -> bool {
        self.ui.wants_cursor_free()
    }

    /// Phase 2.5 M2: returns `true` if the focused widget
    /// (e.g. a TextInput) claims `Key::Character(_)` events.
    /// Hosts (lorekeeper-side input pump) consult this before
    /// populating `pressed()` / `held()` queries with
    /// character events; when true, those keys are consumed
    /// by the runtime and don't reach game handlers. Per UL
    /// `UI_RUNTIME.md` §2.
    pub fn consumes_character_key(&self) -> bool {
        self.ui.consumes_character_key()
    }

    /// Place the anchor `id` at `point`. A
    /// `Portal { anchor: "<id>" }` in the `.ogh` seats its
    /// subtree there instead of at the slot it was declared in;
    /// paint, hit-testing, occlusion and nesting all follow,
    /// because the anchor resolves into the same
    /// viewport-absolute rect an unanchored portal computes.
    ///
    /// **Anchors are host state, not frame state** — they persist
    /// until changed or cleared, so chrome pinned to something
    /// that rarely moves costs nothing per frame. Chrome that
    /// follows the pointer just calls this every frame; setting
    /// the same point twice is a no-op.
    ///
    /// World-anchored chrome projects world → screen host-side
    /// and passes the result: Ogham does not know what a camera
    /// is. Anchors do **not** survive a hot reload (INTENT §7) —
    /// a host that sets an anchor once must re-set it after one.
    pub fn set_anchor(&mut self, id: impl Into<String>, point: widget::point::Point) {
        self.ui.set_anchor(id, point);
    }

    /// Drop the anchor `id`. Portals naming it render nothing
    /// until it is set again — the honest behaviour for "the
    /// thing I was pointing at is gone". Idempotent.
    pub fn clear_anchor(&mut self, id: &str) {
        self.ui.clear_anchor(id);
    }

    /// Drop every anchor at once, for a host tearing down a
    /// screen whose anchors all became meaningless together.
    pub fn clear_anchors(&mut self) {
        self.ui.clear_anchors();
    }

    /// The point currently set for anchor `id`, if any.
    pub fn anchor(&self, id: &str) -> Option<widget::point::Point> {
        self.ui.anchor(id)
    }

    /// Phase 3 M1: dispatch `drag_start` on `origin` with the
    /// given payload + cursor position. Returns the seeded
    /// [`widget::event::DragState`] so the host's input pump
    /// can thread it through subsequent `dispatch_drag_move` /
    /// `dispatch_drag_end` calls.
    pub fn dispatch_drag_start(
        &mut self,
        origin: widget::WidgetRef,
        payload: runtime::value::Value,
        point: widget::point::Point,
    ) -> widget::event::DragState {
        self.ui.dispatch_drag_start(origin, payload, point)
    }

    /// Phase 3 M1: dispatch `drag_move` on the deepest widget
    /// under `point`. Updates `state.current_position` in place.
    /// Returns the widget that received the event, if any.
    pub fn dispatch_drag_move(
        &mut self,
        state: &mut widget::event::DragState,
        point: widget::point::Point,
    ) -> Option<widget::WidgetRef> {
        self.ui.dispatch_drag_move(state, point)
    }

    /// Phase 3 M1: dispatch `drag_end`. Walks portal layers
    /// then the base tree to find the deepest widget whose
    /// `accepts_drop(payload)` is true; fires `drag_end` on
    /// that widget. Falls back to the originator if no target
    /// accepts. Returns the widget that received `drag_end`.
    pub fn dispatch_drag_end(
        &mut self,
        state: &mut widget::event::DragState,
        point: widget::point::Point,
    ) -> Option<widget::WidgetRef> {
        self.ui.dispatch_drag_end(state, point)
    }

    /// Phase 3 M1: drop-target hit-test. Walks portal layers
    /// (high priority → low) then the base tree, returning the
    /// deepest widget at `point` whose `accepts_drop(payload)`
    /// returns true. Used by `dispatch_drag_end` and exposed
    /// for hosts that want to drive drop-zone highlighting
    /// based on the current cursor position during a drag.
    pub fn hit_test_drop_target(
        &self,
        payload: &runtime::value::Value,
        point: &widget::point::Point,
    ) -> Option<widget::WidgetRef> {
        self.ui.hit_test_drop_target(payload, point)
    }

    /// Phase 3 M2: dispatch a `contextmenu` event on the
    /// deepest widget at `point`. Hosts wire this to right-
    /// click in their input pump; the event is distinct from
    /// `mouse_down`/`mouse_up`. Returns `true` if a listener
    /// fired.
    pub fn dispatch_contextmenu(&mut self, point: widget::point::Point) -> bool {
        self.ui.dispatch_contextmenu(point)
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

    /// The font families registered on this instance, in registration
    /// order.
    ///
    /// A document names a family and ogham falls back to a system face
    /// for one it does not know — silently, because a missing font is not
    /// an error. So "the typefaces are registered" is a thing a host has
    /// to be able to assert, and it could not.
    pub fn registered_font_families(&self) -> Vec<String> {
        self.registered_typefaces
            .iter()
            .map(|(family, _)| family.clone())
            .collect()
    }

    /// The loaded module's resolved schema — its records, its
    /// `host_state {}`, its declared events and its `screen` blocks.
    ///
    /// `None` when no module is loaded, or when the module does not
    /// resolve; a document that will not compile has a real error waiting
    /// with real diagnostics, and a host checking its screen ids against a
    /// route table should not bury that under a second complaint.
    /// Resolved against the whole import graph, so a document split across
    /// files answers the same as one written in a single file
    /// (`APPLICATION_BUILD.md` WP-3.1).
    pub fn module_schema(&self) -> Option<runtime::schema::ModuleSchema> {
        self.with_runtime_mut(|rt| rt.module_schema())
    }

    /// Every unrecognised key and value the builder has met, when the
    /// config asked for
    /// [`with_strict_vocabulary`](runtime::config::RuntimeConfig::with_strict_vocabulary).
    /// Empty otherwise, and empty is the answer a repo's test wants.
    ///
    /// De-duplicated, so a mistake in a widget rebuilt every frame is one
    /// entry rather than one per frame, and it survives a hot reload —
    /// which is the moment an author is most likely to be reading it.
    pub fn vocabulary_violations(&self) -> Vec<widget::vocabulary::Violation> {
        self.config.vocabulary_violations()
    }

    /// The event names this instance has handlers registered for.
    ///
    /// The other half of what a document's `events {}` block declares —
    /// see [`route::Chrome::validate_raises`], which is the one caller
    /// that matters.
    pub fn registered_event_names(&self) -> Vec<String> {
        self.with_runtime_mut(|rt| rt.event_handler_names())
    }

    /// Set a default font family that will be used by all text widgets
    /// that don't explicitly specify a `font` in their style.
    pub fn set_default_font(&mut self, name: &str) {
        self.default_font = Some(name.to_string());
        self.ui.set_default_font(name.to_string());
    }

    /// Resolve `Image { path }` lookups against `root` instead of the
    /// default `data/assets/` under the process working directory. Like
    /// registered fonts, the root survives hot reloads.
    pub fn set_image_root(&mut self, root: impl Into<PathBuf>) {
        let root = root.into();
        self.ui.image_cache.set_root(root.clone());
        self.image_root = Some(root);
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

    /// One standard frame: hot-reload check (state-preserving), screen-size
    /// update, re-execute the module if a rerender is pending, tick
    /// animations, lay out. `layout` early-exits when nothing is dirty and
    /// the dimensions are unchanged, so an idle frame does no tree work.
    ///
    /// Unlike [`Self::tick`], there is no inject callback: push host state
    /// beforehand via [`Self::with_runtime_mut`] /
    /// [`runtime::Runtime::set_host_state`] whenever it changes — reload
    /// carries live host state forward, so injection order is safe across
    /// frames.
    ///
    /// Returns a [`FrameReport`]: whether the module re-executed, and
    /// whether a watched edit was reloaded.
    pub fn frame(
        &mut self,
        width: f32,
        height: f32,
        dt: f32,
    ) -> Result<FrameReport, runtime::error::RuntimeError> {
        let reloaded = self.check_for_changes();
        if reloaded {
            self.reload()?;
        }
        self.set_screen_size(width, height);
        let rerendered = self.update()?;
        self.ui.tick_animations(dt);
        self.ui.layout(width, height);
        Ok(FrameReport {
            reloaded,
            rerendered,
        })
    }

    /// Perform a complete frame update: check for file changes, reload if
    /// needed, run the `inject` callback to push host state into the
    /// runtime, then reconcile the widget tree if a rerender is pending.
    /// Hosts that push state as it changes (rather than per frame) can use
    /// [`Self::frame`], which also animates and lays out.
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

    /// Begin exiting the entire UI tree. Cascades [`Widget::begin_exit`]
    /// from the root. Returns `true` if at least one widget has an exit
    /// animation in flight (the host should keep ticking this Ogham and
    /// poll [`Self::is_exit_complete_root`]) or `false` if there was
    /// nothing to animate (the host can drop / replace this Ogham
    /// immediately).
    ///
    /// Used by host-side orchestrators that sequence transitions
    /// between multiple Ogham instances (route swaps, modal stacks).
    pub fn begin_exit_root(&mut self) -> bool {
        let root = self.ui.root.clone();
        let mut g = root.lock().expect("widget lock poisoned");
        g.begin_exit()
    }

    /// Cancel a previously-started exit so the tree returns to its
    /// declared state. Cascades to descendants. Idempotent — harmless
    /// if no exit was in flight. Used when the user reverts a
    /// transition mid-flight (e.g., reopens an overlay before its
    /// close animation finished).
    pub fn cancel_exit_root(&mut self) {
        let root = self.ui.root.clone();
        let mut g = root.lock().expect("widget lock poisoned");
        g.cancel_exit();
    }

    /// True once every in-flight exit animation has settled. Always
    /// `false` before [`Self::begin_exit_root`] is called. The host
    /// orchestrator polls this each frame and finalizes the
    /// transition once it returns true.
    pub fn is_exit_complete_root(&self) -> bool {
        let root = self.ui.root.clone();
        let g = root.lock().expect("widget lock poisoned");
        g.is_exit_complete()
    }

    /// Re-seed every widget that declared `initial:` back to its
    /// initial style and retarget springs toward `declared_style`.
    /// Call when promoting a previously-mounted Ogham to active so
    /// its entry animations replay. Widgets without `initial:` (or
    /// without enabled transitions) are no-ops.
    ///
    /// Also requests a rerender so any widgets that were dropped from
    /// the tree during the prior exit (children with no `exit:`,
    /// drained immediately on `begin_exit`) get re-mounted from the
    /// module's declarative tree on the host's next tick — without
    /// this the tree would render with gaps until something else
    /// happened to dirty host_state.
    pub fn restart_entry_animations(&mut self) {
        let root = self.ui.root.clone();
        let mut g = root.lock().expect("widget lock poisoned");
        g.restart_entry_animation();
        drop(g);
        self.ui.mark_needs_layout();
        let rt = self.runtime.clone();
        rt.lock().expect("runtime lock poisoned").request_rerender();
    }

    /// Cancel any in-flight drag preview on this Ogham. Used by the
    /// host orchestrator before swapping the active Ogham so a drag
    /// originated from the outgoing UI doesn't leak across the
    /// transition. Idempotent.
    pub fn cancel_active_drag(&mut self) {
        self.ui.clear_active_drag();
    }

    /// If the runtime has flagged a rerender, re-execute the module,
    /// bridge the resulting widget values into the widget tree, and
    /// reconcile. Returns `true` if a rerender was performed.
    pub fn update(&mut self) -> Result<bool, runtime::error::RuntimeError> {
        let (widget_value, registry) = {
            let mut rt = self.runtime.lock().expect("runtime lock poisoned");
            if !rt.needs_rerender() {
                return Ok(false);
            }
            let value = rt.rerender()?;
            (value, rt.widget_registry.clone())
        };

        let widget_ref =
            widget::builder::widget_value_to_widget_ref(&registry, &self.runtime, &widget_value)?;

        // Self-heal a stranded host-orchestrated exit. `begin_exit_root` puts the
        // root into an exiting state that only `restart_entry_animations` /
        // `cancel_exit_root` clears; a host that exits this Ogham and later
        // re-shows it without that paired call would otherwise reconcile into a
        // blank ghost (a common orchestrator foot-gun). Reaching here means a
        // rerender is pending — the host is actively pushing this UI's live
        // content — so if a prior root-exit has *fully completed* we clear it now
        // and let the reconcile below re-mount the live tree. Gated on
        // `is_exit_complete_root` so an in-flight exit is never disturbed, and
        // only the root can be exiting this way (normal in-tree / Presence exits
        // leave the root un-exiting), so this can't interfere with element-level
        // exit animations.
        if self.is_exit_complete_root() {
            self.cancel_exit_root();
        }

        // Reconcile reports whether anything in the new tree actually
        // differed from the cached widgets. When the module re-executes
        // because of a host_state change but produces identical widget
        // output, both flags are false and we skip the layout/repaint
        // invalidation entirely — the previous frame's layout stays valid.
        let result = self.ui.reconcile(widget_ref);
        if result.needs_layout {
            self.ui.mark_needs_layout();
        } else if result.needs_repaint {
            self.ui.mark_needs_repaint();
        }
        Ok(true)
    }
}
