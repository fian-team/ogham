//! Ogham runtime: bytecode compiler, VM, and supporting infrastructure.
//!
//! The primary execution path compiles Ogham source to bytecode
//! ([`compiler`]) and runs it in a stack-based VM ([`vm`]).
//! Shared arithmetic and comparison operations live in [`ops`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::parser::{Function, ImportStatement, Parser};
use crate::runtime::compiler::Compiler;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, VMError};
use crate::runtime::opcode::{FunctionProto, VMClosure};
use crate::runtime::value::Value;
use crate::runtime::vm::VM;
use crate::scanner::Scanner;
use crate::widget::builder::WidgetRegistry;

pub mod compiler;
pub mod config;
pub mod environment;
pub mod error;
pub mod host_state;
pub mod opcode;
pub mod ops;
pub mod schema;
pub mod value;
pub mod vm;
pub mod descriptor;

pub use host_state::{HostStateSink, HostStateSinkExt, IntoHostValue};

/// Built-in prelude source that defines helper functions available in all modules.
/// Compiled and executed once when the runtime is first created via
/// `Runtime::from_source`/`Runtime::from_file`.
///
/// Function parameters require type annotations per `parse_function`, so the
/// prelude declares each colour component as `int`. The annotations are
/// hints only — the runtime never enforces them — so the same body still
/// accepts the floats and ints callers pass.
const PRELUDE_SOURCE: &str = r#"
let rgb = fn (r: int, g: int, b: int) { { r: r, g: g, b: b, a: 255 } };
let rgba = fn (r: int, g: int, b: int, a: int) { { r: r, g: g, b: b, a: a } };
"#;

/// Per-effect slot stored in `StateManager.effects`. `previous_deps`
/// is `None` until the first fire; `pending_cleanup` is set by an
/// `effect` body that registers a `cleanup` block, and consumed at
/// the next dep change or path unmount.
#[allow(dead_code)] // M0 declares the shape; M2 reads previous_deps + closure.
pub(crate) struct EffectSlot {
    pub previous_deps: Option<Vec<Value>>,
    pub pending_cleanup: Option<Rc<VMClosure>>,
    pub closure: Rc<VMClosure>,
}

/// Manages component state, the call stack used for state key
/// generation, and Phase 2 lifecycle bookkeeping.
///
/// Phase 2 (lifecycle hooks + Portal) adds:
/// - `previous_active_paths`: snapshot of `active_state_paths` from
///   the previous render, used to compute mount/unmount diffs.
/// - `unmount_hooks` / `effects`: persistent registries keyed by
///   `(path, hook_id)`. Mount has no persistent map — it queues
///   inline at opcode time.
/// - `pending_*` queues: per-frame work items drained at pre-layout
///   (unmounts, effect cleanups) or post-layout (mounts, effect
///   fires) by the renderer.
/// - `candidate_unmounts`: paths that have stopped being visited
///   but whose owning widget is still in the tree (ghost during
///   exit animation). Resolved when `drain_exited_children` removes
///   the widget — see `Widget::flush_lifecycle_on_drain`.
pub(crate) struct StateManager {
    pub(crate) component_state: HashMap<String, Value>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) active_state_paths: HashSet<String>,
    pub(crate) has_branched: bool,
    pub(crate) call_counters: HashMap<String, usize>,

    // -- Phase 2 lifecycle --
    // Pending queues carry the closure with each entry rather
    // than just the key. This means flush_for_path_prefix can
    // remove the entry from the persistent map AND queue it for
    // drain in one step — the drainer doesn't need to keep the
    // map entry alive while it consumes the queue.
    pub(crate) previous_active_paths: HashSet<String>,
    pub(crate) unmount_hooks: HashMap<(String, u16), Rc<VMClosure>>,
    pub(crate) effects: HashMap<(String, u16), EffectSlot>,
    pub(crate) pending_mounts: Vec<(String, u16, Rc<VMClosure>)>,
    pub(crate) pending_unmounts: Vec<(String, u16, Rc<VMClosure>)>,
    #[allow(dead_code)]
    pub(crate) pending_effect_fires: Vec<(String, u16)>,
    pub(crate) pending_effect_cleanups: Vec<(String, u16, Rc<VMClosure>)>,
    pub(crate) candidate_unmounts: HashSet<String>,
}

impl StateManager {
    fn new() -> Self {
        Self {
            component_state: HashMap::new(),
            call_stack: Vec::new(),
            active_state_paths: HashSet::new(),
            has_branched: false,
            call_counters: HashMap::new(),

            previous_active_paths: HashSet::new(),
            unmount_hooks: HashMap::new(),
            effects: HashMap::new(),
            pending_mounts: Vec::new(),
            pending_unmounts: Vec::new(),
            pending_effect_fires: Vec::new(),
            pending_effect_cleanups: Vec::new(),
            candidate_unmounts: HashSet::new(),
        }
    }

    /// Rotate the active-paths set: move current → previous and
    /// clear current. Called at frame start in `Runtime::rerender`.
    /// The diff `mounted = current − previous`, `unmounted =
    /// previous − current` is computed by callers as needed.
    pub(crate) fn rotate_active_paths(&mut self) {
        std::mem::swap(&mut self.active_state_paths, &mut self.previous_active_paths);
        self.active_state_paths.clear();
    }

    /// Phase 2 lifecycle: invoked when a widget owning the given
    /// path prefix has fully drained from the tree (after any exit
    /// animation has settled). Walks the persistent registries
    /// (`unmount_hooks`, `effects`) and queues any matching entries
    /// onto the pending drain queues for the next frame's
    /// pre-layout step. Removes the entries from the persistent
    /// maps so they don't fire again.
    ///
    /// Empty prefix is a no-op — the empty-string prefix would
    /// match every entry, which is never what a drain should do.
    ///
    /// M0 ships this helper and the synthetic-state tests; M1
    /// wires it into `drain_exited_children` once hook
    /// registration is in place.
    pub(crate) fn flush_for_path_prefix(&mut self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        // Move matching unmount_hooks into the pending queue.
        // Carrying the closure in the queue lets the drainer fire
        // it without consulting the persistent map.
        let unmount_keys: Vec<(String, u16)> = self
            .unmount_hooks
            .keys()
            .filter(|(p, _)| p.starts_with(prefix))
            .cloned()
            .collect();
        for key in unmount_keys {
            if let Some(closure) = self.unmount_hooks.remove(&key) {
                self.pending_unmounts.push((key.0, key.1, closure));
            }
        }
        // Move matching effect cleanups into the cleanup queue,
        // then drop the slot. M2 also queues an effect-fire if
        // appropriate; M1 only handles the unmount-cleanup case.
        let effect_keys: Vec<(String, u16)> = self
            .effects
            .keys()
            .filter(|(p, _)| p.starts_with(prefix))
            .cloned()
            .collect();
        for key in effect_keys {
            if let Some(slot) = self.effects.remove(&key) {
                if let Some(cleanup) = slot.pending_cleanup {
                    self.pending_effect_cleanups
                        .push((key.0, key.1, cleanup));
                }
            }
        }
        // Clear any candidate_unmounts entries for this prefix.
        self.candidate_unmounts.retain(|p| !p.starts_with(prefix));
    }

    /// Phase 2 lifecycle: cancel a widget's pending unmount when
    /// `cancel_exit` re-mounts it. Removes any candidate_unmount
    /// entries whose path starts with the given prefix.
    ///
    /// M0 ships this helper; M1 wires it into `cancel_exit`.
    #[allow(dead_code)]
    pub(crate) fn cancel_unmount_for_prefix(&mut self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        self.candidate_unmounts.retain(|p| !p.starts_with(prefix));
    }

    /// Get the current call stack path as a string.
    pub(crate) fn get_call_stack_path(&self) -> String {
        if self.call_stack.is_empty() {
            "".to_string()
        } else {
            self.call_stack.join("/")
        }
    }

    /// Generate a state key from the current call stack path and variable name.
    pub(crate) fn get_state_key(&self, variable_name: &str) -> String {
        let path = self.get_call_stack_path();
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path, variable_name)
        }
    }

    /// Build a key from a given path slice and variable name.
    fn make_key(path: &[String], variable_name: &str) -> String {
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path.join("/"), variable_name)
        }
    }

    /// Search up the call stack for an existing state key.
    fn find_existing_key(&self, variable_name: &str) -> Option<String> {
        let mut search_path = self.call_stack.clone();
        loop {
            let key = Self::make_key(&search_path, variable_name);
            if self.component_state.contains_key(&key) {
                return Some(key);
            }
            if search_path.is_empty() {
                break;
            }
            search_path.pop();
        }
        None
    }

    /// Get state value for a variable, searching up the call stack.
    pub(crate) fn get_state_value(&self, variable_name: &str) -> Option<Value> {
        self.find_existing_key(variable_name)
            .and_then(|key| self.component_state.get(&key).cloned())
    }

    /// Set state value for a variable at the most specific existing path,
    /// or create it at the current path if it doesn't exist.
    pub(crate) fn set_state_value(&mut self, variable_name: &str, value: Value) {
        if let Some(key) = self.find_existing_key(variable_name) {
            self.component_state.insert(key, value);
        } else {
            let key = self.get_state_key(variable_name);
            self.component_state.insert(key, value);
        }
    }

    /// Cleanup state for components that are no longer mounted.
    /// Top-level state (empty path) is always considered active — the module
    /// itself is always "mounted", so state declared at module scope persists
    /// for the runtime's lifetime.
    fn cleanup_unmounted_state(&mut self) {
        let keys_to_remove: Vec<String> = self
            .component_state
            .keys()
            .filter(|key| {
                if let Some(colon_pos) = key.find(':') {
                    let path = &key[..colon_pos];
                    if path.is_empty() {
                        return false;
                    }
                    !self.active_state_paths.contains(path)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        for key in keys_to_remove {
            self.component_state.remove(&key);
        }
    }
}

/// Manages import resolution, caching, and cycle detection.
pub(crate) struct ImportResolver {
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) import_paths: HashMap<String, PathBuf>,
    loading_stack: Vec<PathBuf>,
    loaded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, Environment>,
}

impl ImportResolver {
    fn new() -> Self {
        Self {
            project_root: None,
            import_paths: HashMap::new(),
            loading_stack: Vec::new(),
            loaded: HashSet::new(),
            cache: HashMap::new(),
        }
    }

    /// Returns the canonical paths of all modules that were imported.
    pub(crate) fn get_imported_paths(&self) -> Vec<PathBuf> {
        self.loaded.iter().cloned().collect()
    }
}

/// The Ogham runtime: holds module state, component state, import caches,
/// and the environment used during execution.
pub struct Runtime {
    pub(crate) environment: Environment,
    host_state: HashMap<String, Value>,
    event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>>,
    needs_rerender: bool,
    module: Option<Function>,
    /// Compiled bytecode for the module (cached so rerenders skip compilation).
    compiled_module: Option<FunctionProto>,
    pub(crate) state: StateManager,
    pub(crate) imports: ImportResolver,
    /// Screen dimensions set from the most recent `layout()` call.
    /// Exposed as built-in variables `screen_width` and `screen_height` in the VM.
    pub(crate) screen_width: f32,
    pub(crate) screen_height: f32,
    /// Registry of widget type names to factory functions. Populated with
    /// built-in types by default; host applications can add custom widgets
    /// via [`RuntimeConfig::with_widget`].
    pub widget_registry: WidgetRegistry,
    /// Stack of `(name, value)` entries used by the `Context` primitive. The
    /// compiler emits `PushContext` / `PopContext` around a Context widget's
    /// children evaluation; `use_context("name")` walks this stack.
    ///
    /// Transient — cleared at the start of every module execution.
    pub(crate) context_stack: Vec<(String, Value)>,
    /// Phase 2 lifecycle: `true` if the compiled module emits any
    /// of the lifecycle opcodes (RegisterMountHook /
    /// RegisterUnmountHook / RegisterEffect /
    /// RegisterEffectCleanup) anywhere in its bytecode tree. Set
    /// by the bytecode scan in `set_compiled_module`. Used by the
    /// VM `Call` opcode handler to gate the per-call path-marking
    /// — modules without hooks pay zero per-call overhead.
    pub(crate) lifecycle_active: bool,
    /// Phase 2 lifecycle: per-frame log of stringified errors
    /// from hook bodies. Hook errors are logged and execution
    /// continues with the next hook in the queue (per design's
    /// "log-and-continue" policy). Capacity-bounded ring buffer
    /// (oldest entries dropped at LIFECYCLE_LOG_CAPACITY).
    /// Cleared at frame start.
    pub(crate) lifecycle_error_log: Vec<String>,
}

/// Maximum number of hook-error entries retained in
/// `Runtime::lifecycle_error_log`. Per impl-plan decision #4
/// (M0 kickoff), 100 is the default and not configurable in M1.
pub(crate) const LIFECYCLE_LOG_CAPACITY: usize = 100;

impl Runtime {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
            needs_rerender: false,
            module: None,
            compiled_module: None,
            state: StateManager::new(),
            imports: ImportResolver::new(),
            screen_width: 0.0,
            widget_registry: WidgetRegistry::with_defaults(),
            screen_height: 0.0,
            context_stack: Vec::new(),
            lifecycle_active: false,
            lifecycle_error_log: Vec::new(),
        }
    }

    /// Phase 2: number of hook-body errors logged in the current
    /// frame. Reset to 0 at frame start. Hosts can surface this
    /// in debug overlays (UL audit OQ#1 use case).
    pub fn lifecycle_error_count(&self) -> usize {
        self.lifecycle_error_log.len()
    }

    /// Phase 2: a snapshot of the current frame's hook-body
    /// error messages. Latest at the end. Capped at
    /// LIFECYCLE_LOG_CAPACITY entries.
    pub fn lifecycle_error_log(&self) -> &[String] {
        &self.lifecycle_error_log
    }

    /// Phase 2: append an error to the per-frame log, dropping
    /// the oldest entry if capacity is reached.
    pub(crate) fn log_lifecycle_error(&mut self, msg: impl Into<String>) {
        if self.lifecycle_error_log.len() >= LIFECYCLE_LOG_CAPACITY {
            self.lifecycle_error_log.remove(0);
        }
        self.lifecycle_error_log.push(msg.into());
    }

    /// Phase 2: drain `pending_unmounts` and
    /// `pending_effect_cleanups` from this frame, calling each
    /// closure with the runtime's current scope. Errors are
    /// logged to `lifecycle_error_log` (one entry per failure)
    /// and execution continues with the next entry. Order:
    /// unmounts deepest-first (sorted by path length descending),
    /// then effect cleanups in registration order (M2 cares
    /// about ordering; M1 has none of these to drain).
    ///
    /// Hosts call this *before* `UI::layout()`. M1's caller is
    /// `Ogham::tick()` (in lib.rs), which sequences the new
    /// per-frame lifecycle dispatch around the existing
    /// reconcile→tick_animations→layout pipeline.
    pub fn pre_layout_drain(&mut self) {
        // Sort deepest-first so a parent's unmount fires after
        // its children's. Path length is a stand-in for depth
        // (paths are joined with "/" so depth = number of "/" + 1
        // — but the byte length grows monotonically with depth
        // for any single tree, which is sufficient for ordering
        // siblings vs. parents).
        self.state
            .pending_unmounts
            .sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        let unmounts = std::mem::take(&mut self.state.pending_unmounts);
        for (path, hook_id, closure) in unmounts {
            if let Err(e) = self.call_bytecode_closure(&closure, &[]) {
                self.log_lifecycle_error(format!(
                    "on_unmount at {}#{}: {:?}",
                    path, hook_id, e
                ));
            }
        }
        // M1: pending_effect_cleanups is always empty (effects
        // ship in M2). Drain unconditionally so the M2 wiring
        // is just "fill the queue."
        let cleanups = std::mem::take(&mut self.state.pending_effect_cleanups);
        for (path, hook_id, closure) in cleanups {
            if let Err(e) = self.call_bytecode_closure(&closure, &[]) {
                self.log_lifecycle_error(format!(
                    "effect cleanup at {}#{}: {:?}",
                    path, hook_id, e
                ));
            }
        }
    }

    /// Phase 2: drain `pending_mounts` and `pending_effect_fires`
    /// from this frame, calling each closure with the runtime's
    /// current scope. Mounts run parents-first (sorted by path
    /// length ascending) so a parent's `on_mount` can read state
    /// its children populated.
    ///
    /// Hosts call this *after* `UI::layout()` so mount bodies
    /// can read post-layout sizes (per design §"Hook firing
    /// timing").
    pub fn post_layout_drain(&mut self) {
        self.state
            .pending_mounts
            .sort_by(|a, b| a.0.len().cmp(&b.0.len()));
        let mounts = std::mem::take(&mut self.state.pending_mounts);
        for (path, hook_id, closure) in mounts {
            if let Err(e) = self.call_bytecode_closure(&closure, &[]) {
                self.log_lifecycle_error(format!(
                    "on_mount at {}#{}: {:?}",
                    path, hook_id, e
                ));
            }
        }
        // M2 fills in pending_effect_fires; M1 leaves it empty.
        let fires = std::mem::take(&mut self.state.pending_effect_fires);
        for _key in fires {
            // No-op in M1.
        }
    }

    /// Phase 2: M1 path-disappear unmount semantics. After VM
    /// execution, walk paths in `previous_active_paths` that are
    /// no longer in `active_state_paths` and queue their
    /// registered `on_unmount` hooks for the next pre-layout
    /// drain. Removes the entries from `unmount_hooks`.
    ///
    /// LIMITATION (documented in M1): this fires unmount
    /// immediately when a path stops being visited, not after
    /// the widget's exit animation drains. The full drain-time
    /// semantics from the design require threading mutable
    /// runtime state through the widget tree's tick_animations
    /// chain — invasive surgery deferred to a later merge
    /// (likely M3 when Portal needs it for focus stack
    /// stability). For the M5 canonical deliverables (Settings
    /// save-on-close, escape menu Portal), path-disappear is
    /// sufficient.
    pub(crate) fn queue_disappeared_unmounts(&mut self) {
        let disappeared: Vec<String> = self
            .state
            .previous_active_paths
            .difference(&self.state.active_state_paths)
            .cloned()
            .collect();
        for path in disappeared {
            // Use flush_for_path_prefix with the full path as
            // prefix — for a path "panel/foo", this catches any
            // hook key starting with "panel/foo" (children of the
            // unmounted function). The same path is rarely a
            // prefix of an unrelated longer path because path
            // components are function names plus call counters.
            self.state.flush_for_path_prefix(&path);
        }
    }

    /// Phase 2: walk a compiled FunctionProto (and its sub-protos
    /// recursively) and return `true` if any of the four lifecycle
    /// opcodes appears anywhere. Called by `set_compiled_module`
    /// to compute `lifecycle_active`.
    fn bytecode_uses_lifecycle(proto: &FunctionProto) -> bool {
        for op in &proto.chunk.code {
            match op {
                opcode::OpCode::RegisterMountHook(_)
                | opcode::OpCode::RegisterUnmountHook(_)
                | opcode::OpCode::RegisterEffect { .. }
                | opcode::OpCode::RegisterEffectCleanup => return true,
                _ => {}
            }
        }
        for sub in &proto.protos {
            if Self::bytecode_uses_lifecycle(sub) {
                return true;
            }
        }
        false
    }

    /// Phase 2: cache the compiled module and update the
    /// lifecycle_active flag based on its bytecode. Use this
    /// instead of assigning `compiled_module` directly so the
    /// flag stays in sync with the compiled artifact.
    fn set_compiled_module(&mut self, proto: FunctionProto) {
        self.lifecycle_active = Self::bytecode_uses_lifecycle(&proto);
        self.compiled_module = Some(proto);
    }

    pub fn set_project_root(&mut self, path: PathBuf) {
        self.imports.project_root = Some(path);
    }

    pub fn project_root(&self) -> Option<&PathBuf> {
        self.imports.project_root.as_ref()
    }

    pub fn set_import_paths(&mut self, paths: HashMap<String, PathBuf>) {
        self.imports.import_paths = paths;
    }

    /// Update the screen dimensions exposed as built-in variables.
    /// Called from `UI::layout()` each frame.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Read an Ogham state variable by name. Returns `None` if the variable
    /// doesn't exist in the component state.
    pub fn get_state(&self, name: &str) -> Option<Value> {
        self.state.get_state_value(name)
    }

    pub fn inject_host_state(&mut self, name: String, value: Value) {
        self.host_state.insert(name, value);
    }

    /// Like `inject_host_state`, but only inserts the value when it differs
    /// from the currently stored value (avoiding unnecessary HashMap churn
    /// on every frame when state hasn't changed). Triggers a rerender when
    /// the value actually changed, matching the contract of
    /// `inject_host_state_batch`.
    pub fn inject_host_state_if_changed(&mut self, name: String, value: Value) {
        if self.host_state.get(&name) != Some(&value) {
            self.host_state.insert(name, value);
            self.request_rerender();
        }
    }

    /// Push a single host-state binding using the [`IntoHostValue`] trait
    /// to pick the right `Value` variant. Skips the insert and the rerender
    /// when the new value equals the stored one, and avoids allocating a
    /// `String` for the key on the no-change path. Prefer this over the
    /// hand-rolled `inject_host_state_batch([...])` shape for binding sites
    /// that build state from a Rust struct.
    pub fn set_host_state(&mut self, name: &str, value: impl IntoHostValue) {
        self.set_host_state_value(name, value.into_host_value());
    }

    /// `Value`-typed inner of [`set_host_state`]. Kept separate so the
    /// [`HostStateSink`] trait can be object-safe.
    pub(crate) fn set_host_state_value(&mut self, name: &str, value: Value) {
        if self.host_state.get(name) != Some(&value) {
            self.host_state.insert(name.to_string(), value);
            self.request_rerender();
        }
    }

    pub fn get_host_state(&self, name: &str) -> Option<Value> {
        self.host_state.get(name).cloned()
    }

    /// Inject multiple host state values at once. Only values that differ
    /// from the currently stored value are inserted, and `request_rerender`
    /// is called automatically if anything changed.
    pub fn inject_host_state_batch(
        &mut self,
        values: impl IntoIterator<Item = (String, Value)>,
    ) {
        let mut changed = false;
        for (key, value) in values {
            if self.host_state.get(&key) != Some(&value) {
                self.host_state.insert(key, value);
                changed = true;
            }
        }
        if changed {
            self.request_rerender();
        }
    }

    pub fn register_event_handler<S, F>(&mut self, name: S, handler: F)
    where
        S: Into<String>,
        F: Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
    }

    pub(crate) fn register_event_handler_arc(
        &mut self,
        name: String,
        handler: Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>,
    ) {
        self.event_handlers.insert(name, handler);
    }

    /// Dispatch an event to the registered handler, if any.
    ///
    /// Returns `Ok(Value::Void)` when no handler is registered for the name —
    /// fire-and-forget `event()` calls treat a missing handler as a no-op.
    /// For tracked mutations, the caller distinguishes success/error via the
    /// returned `Result`.
    pub fn emit_event(&self, name: &str, args: &[Value]) -> Result<Value, String> {
        match self.event_handlers.get(name) {
            Some(handler) => handler(args),
            None => Ok(Value::Void),
        }
    }

    pub fn needs_rerender(&self) -> bool {
        self.needs_rerender
    }

    pub fn request_rerender(&mut self) {
        self.needs_rerender = true;
    }

    pub fn set_module(&mut self, module: Function) {
        self.module = Some(module);
        // Invalidate cached bytecode so the next execution recompiles.
        self.compiled_module = None;
        // Reset lifecycle_active too — it'll be recomputed when the
        // new module compiles. Without this, swapping from a
        // hook-using module to a hookless one would leave the flag
        // stuck at true.
        self.lifecycle_active = false;
    }

    pub fn get_module(&self) -> Option<&Function> {
        self.module.as_ref()
    }

    /// Returns the canonical paths of all modules that were imported during the last
    /// execute_module/rerender. Used by the file watcher to watch every file that
    /// affects the current UI.
    pub fn get_imported_paths(&self) -> Vec<PathBuf> {
        self.imports.get_imported_paths()
    }

    pub fn rerender(&mut self) -> Result<Value, VMError> {
        if self.module.is_none() {
            return Err(VMError::InvalidOperation(
                "No module stored in runtime. Cannot rerender.".to_string(),
            ));
        }
        self.needs_rerender = false;
        self.environment = Environment::new();
        self.state.rotate_active_paths();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        // Phase 2: clear the per-frame error log. Previous frame's
        // hook errors are no longer relevant.
        self.lifecycle_error_log.clear();
        let result = self.execute_module_cached();
        // Phase 2: compute disappeared paths and queue their
        // unmount hooks for drain. Then drain all pending hook
        // queues. M1 path-disappear semantics; M3+ refines to
        // drain-time once Portal needs it for focus stack.
        self.queue_disappeared_unmounts();
        self.pre_layout_drain();
        self.post_layout_drain();
        self.state.cleanup_unmounted_state();
        result
    }

    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        self.state.rotate_active_paths();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        self.imports.loading_stack.clear();
        self.imports.loaded.clear();
        self.imports.cache.clear();

        // Compile (or use cached compilation).
        let proto = if let Some(ref cached) = self.compiled_module {
            cached.clone()
        } else {
            let proto = Compiler::compile_module(module)?;
            // set_compiled_module also updates lifecycle_active.
            self.set_compiled_module(proto.clone());
            proto
        };

        // Phase 2: clear the per-frame error log so first-render
        // hook errors are surfaced cleanly.
        self.lifecycle_error_log.clear();

        let mut vm = VM::new();
        let result = vm.run(&proto, self);
        // Phase 2: same drain ordering as rerender.
        self.queue_disappeared_unmounts();
        self.pre_layout_drain();
        self.post_layout_drain();
        self.state.cleanup_unmounted_state();
        result
    }

    /// Re-execute a module using cached bytecode and cached imports.
    fn execute_module_cached(&mut self) -> Result<Value, VMError> {
        self.state.active_state_paths.clear();
        self.state.call_stack.clear();
        self.state.call_counters.clear();
        self.imports.loading_stack.clear();
        // Context is transient per render; compiler-emitted Push/Pop pairs
        // balance out, but clear anyway to recover from any imbalance.
        self.context_stack.clear();
        // NOTE: import_loaded and import_cache are intentionally preserved
        // so that imports are resolved from memory rather than disk.

        let proto = self.compiled_module.clone().ok_or_else(|| {
            VMError::InvalidOperation(
                "No compiled module cached. Cannot execute_module_cached.".to_string(),
            )
        })?;

        let mut vm = VM::new();
        let result = vm.run(&proto, self);
        self.state.cleanup_unmounted_state();
        result
    }

    pub(crate) fn execute_import(
        &mut self,
        import_stmt: &ImportStatement,
    ) -> Result<Value, VMError> {
        let project_root = self.imports.project_root.as_ref().ok_or_else(|| {
            VMError::ImportError("project root not set; cannot resolve import path".to_string())
        })?;

        let path_str = import_stmt.get_path();

        let mut resolved = None;
        for (prefix, base) in &self.imports.import_paths {
            if let Some(rest) = path_str.strip_prefix(prefix.as_str()) {
                let rest = rest.strip_prefix('/').unwrap_or(rest);
                resolved = Some(base.join(rest));
                break;
            }
        }
        let mut resolved = resolved.unwrap_or_else(|| project_root.join(path_str));

        if resolved.extension().is_none() {
            resolved.set_extension("ogh");
        }

        let key = resolved.canonicalize().unwrap_or(resolved.clone());

        if self.imports.loading_stack.contains(&key) {
            let mut cycle = self.imports.loading_stack.clone();
            cycle.push(key.clone());
            return Err(VMError::ImportCycle(cycle));
        }
        if self.imports.loaded.contains(&key) {
            if let Some(cached_env) = self.imports.cache.get(&key) {
                let names_to_copy_opt: Option<Vec<String>> = import_stmt.get_names().clone();
                if let Some(ref names) = names_to_copy_opt {
                    for name in names {
                        if cached_env.get(name).is_none() {
                            return Err(VMError::ImportError(format!(
                                "export '{}' not found in {}",
                                name,
                                resolved.display()
                            )));
                        }
                    }
                }
                if let Err(conflict_name) =
                    self.environment
                        .copy_from(cached_env, names_to_copy_opt.as_deref(), false)
                {
                    return Err(VMError::ImportConflict(conflict_name));
                }
            }
            return Ok(Value::Void);
        }

        let source = fs::read_to_string(&resolved).map_err(|e| {
            VMError::ImportError(format!("failed to read {}: {}", resolved.display(), e))
        })?;

        self.imports.loading_stack.push(key.clone());

        let mut scanner = Scanner::new(source);
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        let imported_module = parser.parse().map_err(|e| {
            VMError::ImportError(format!(
                "parse error in {} at {}:{}: {}",
                resolved.display(),
                e.line,
                e.column,
                e.message
            ))
        })?;

        let (proto, local_names) = Compiler::compile_import(&imported_module).map_err(|e| {
            self.imports.loading_stack.pop();
            e
        })?;
        let mut vm = VM::new();
        let _result = vm.run(&proto, self).map_err(|e| {
            self.imports.loading_stack.pop();
            e
        })?;
        let exports_map = vm.read_stack_locals(&local_names);

        let mut temp_env = Environment::new();
        for (name, value) in exports_map {
            temp_env.define(name, value);
        }

        let names_to_copy: Option<Vec<String>> = import_stmt.get_names().clone();
        if let Some(ref names) = names_to_copy {
            for name in names {
                if temp_env.get(name).is_none() {
                    self.imports.loading_stack.pop();
                    return Err(VMError::ImportError(format!(
                        "export '{}' not found in {}",
                        name,
                        resolved.display()
                    )));
                }
            }
        }

        self.imports.cache.insert(key.clone(), temp_env.clone());

        if let Err(conflict_name) =
            self.environment
                .copy_from(&temp_env, names_to_copy.as_deref(), false)
        {
            self.imports.loading_stack.pop();
            return Err(VMError::ImportConflict(conflict_name));
        }

        self.imports.loading_stack.pop();
        self.imports.loaded.insert(key);

        Ok(Value::Void)
    }

    /// Execute the built-in prelude, injecting `rgb` and `rgba` helpers into host state.
    fn execute_prelude(&mut self) -> Result<(), VMError> {
        let mut scanner = Scanner::new(PRELUDE_SOURCE.to_string());
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        let prelude_module = parser.parse().map_err(|e| {
            VMError::InvalidOperation(format!("prelude parse error: {}", e.message))
        })?;
        let proto = Compiler::compile_module(&prelude_module)?;
        let mut vm = VM::new();
        vm.run(&proto, self)?;
        // Move any prelude bindings from environment into host_state
        let env_vars = self.environment.top_level_variables().clone();
        for (name, value) in env_vars {
            self.inject_host_state(name, value);
        }
        self.environment = Environment::new();
        Ok(())
    }

    /// Call a bytecode closure (produced by the bytecode compiler).
    /// Used by the widget tree's event handlers.
    pub fn call_bytecode_closure(
        &mut self,
        closure: &Rc<opcode::VMClosure>,
        args: &[Value],
    ) -> Result<Value, VMError> {
        let mut vm = VM::new();
        vm.call_closure(closure, args, self)
    }

    pub fn from_file<P: AsRef<Path>>(
        path: P,
        config: Option<RuntimeConfig>,
    ) -> Result<Runtime, RuntimeError> {
        let path_buf = path.as_ref().to_path_buf();
        let source = fs::read_to_string(&path_buf)?;
        let mut runtime = Self::from_source(&source, config)?;
        if runtime.project_root().is_none() {
            runtime.set_project_root(
                path_buf
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from(".")),
            );
        }
        Ok(runtime)
    }

    pub fn from_source(
        source: &str,
        config: Option<RuntimeConfig>,
    ) -> Result<Runtime, RuntimeError> {
        let mut scanner = Scanner::new(source.to_string());
        let tokens = scanner.scan();

        let mut parser = Parser::new(tokens);
        let module = parser.parse()?;

        let mut runtime = Runtime::new();

        if let Err(e) = runtime.execute_prelude() {
            eprintln!("[ogham] prelude error: {:?}", e);
        }

        if let Some(config) = config.as_ref() {
            if let Some(ref state) = config.host_state.as_ref() {
                for (name, value) in state.iter() {
                    runtime.inject_host_state(name.clone(), value.clone());
                }
            }

            for (name, handler) in config.event_handlers.iter() {
                runtime.register_event_handler_arc(name.clone(), handler.clone());
            }

            if let Some(ref project_root) = config.project_root {
                runtime.set_project_root(project_root.clone());
            }

            if !config.import_paths.is_empty() {
                runtime.set_import_paths(config.import_paths.clone());
            }

            for (name, factory) in &config.custom_widgets {
                runtime
                    .widget_registry
                    .factories
                    .insert(name.clone(), factory.clone());
            }
        }

        runtime.set_module(module.clone());

        Ok(runtime)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Value {
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        runtime.execute_module(&module).expect("execute")
    }

    #[test]
    fn arithmetic_operations() {
        assert_eq!(
            run("let main = fn () { return 2 + 3; };"),
            Value::Integer(5)
        );
        assert_eq!(
            run("let main = fn () { return 10 - 4; };"),
            Value::Integer(6)
        );
        assert_eq!(
            run("let main = fn () { return 3 * 7; };"),
            Value::Integer(21)
        );
    }

    #[test]
    fn comparison_operations() {
        assert_eq!(
            run("let main = fn () { return 5 > 3; };"),
            Value::Boolean(true)
        );
        assert_eq!(
            run("let main = fn () { return 2 < 1; };"),
            Value::Boolean(false)
        );
        assert_eq!(
            run("let main = fn () { return 3 == 3; };"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn string_concatenation() {
        assert_eq!(
            run(r#"let main = fn () { return "hello" + " " + "world"; };"#),
            Value::String("hello world".to_string())
        );
    }

    #[test]
    fn closures_capture_variables() {
        let source = r#"
let make_adder = fn (x: int) {
    return fn (y: int) { return x + y; };
};
let main = fn () {
    let add5 = make_adder(5);
    return add5(3);
};
"#;
        assert_eq!(run(source), Value::Integer(8));
    }

    #[test]
    fn conditional_if_else() {
        let source = r#"
let main = fn () {
    let x = 10;
    if x > 5 {
        return true;
    } else {
        return false;
    }
};
"#;
        assert_eq!(run(source), Value::Boolean(true));
    }

    #[test]
    fn for_loop_expression_produces_array() {
        let source = r#"
let main = fn () {
    let arr = [1, 2, 3];
    return arr.length();
};
"#;
        assert_eq!(run(source), Value::Integer(3));
    }

    #[test]
    fn match_expression() {
        let source = r#"
let main = fn () {
    let x = 2;
    return match x {
        1 => "one",
        2 => "two",
        _ => "other",
    };
};
"#;
        assert_eq!(run(source), Value::String("two".to_string()));
    }

    #[test]
    fn state_persists_across_rerenders() {
        let source = r#"
let main = fn () {
    state count = 0;
    count = count + 1;
    return count;
};
"#;
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let first = runtime.execute_module(&module).expect("first execute");
        assert_eq!(first, Value::Integer(1));
        let second = runtime.rerender().expect("rerender");
        assert_eq!(second, Value::Integer(2));
    }

    #[test]
    fn map_literals() {
        let source = r#"
let main = fn () {
    let m = { x: 10, y: 20 };
    return m.x + m.y;
};
"#;
        assert_eq!(run(source), Value::Integer(30));
    }

    #[test]
    fn nested_function_calls() {
        let source = r#"
let double = fn (n: int) { return n * 2; };
let main = fn () {
    return double(double(3));
};
"#;
        assert_eq!(run(source), Value::Integer(12));
    }

    #[test]
    fn array_access_length_and_index() {
        let source = r#"
let main = fn () {
  let array = [1, 2, 3, 4, 5];
  let last_index = array.length() - 1;
  let value = array[last_index];
  value
};
"#;
        let mut runtime =
            Runtime::from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let result = runtime.execute_module(&module).expect("execute");
        assert_eq!(result, Value::Integer(5));
    }

    // -----------------------------------------------------------------
    // Phase 2 — M0 lifecycle plumbing tests
    // -----------------------------------------------------------------

    use crate::runtime::opcode::{Chunk, OpCode, VMClosure};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Build a minimal closure for use as a hook stand-in. Real hook
    /// closures are produced by the Closure opcode at runtime;
    /// these synthetic ones are only used to populate the
    /// registries for plumbing tests.
    fn dummy_closure(name: &str, path: Vec<String>) -> Rc<VMClosure> {
        let proto = FunctionProto::new(name.to_string(), 0);
        Rc::new(VMClosure {
            proto: Rc::new(proto),
            upvalues: Vec::new(),
            captured_path: path,
        })
    }

    /// Build a synthetic FunctionProto that emits a single opcode,
    /// for testing the bytecode_uses_lifecycle scan.
    fn proto_with_op(op: OpCode) -> FunctionProto {
        let mut chunk = Chunk::new();
        chunk.emit(op, 1);
        chunk.emit(OpCode::Return, 1);
        let mut proto = FunctionProto::new("synthetic".to_string(), 0);
        proto.chunk = chunk;
        proto
    }

    #[test]
    fn rotate_active_paths_swaps_current_into_previous() {
        let mut sm = StateManager::new();
        sm.active_state_paths.insert("frame_n_path".to_string());
        sm.previous_active_paths.insert("stale".to_string());

        sm.rotate_active_paths();

        assert!(sm.active_state_paths.is_empty(),
            "current should be cleared after rotation");
        assert!(sm.previous_active_paths.contains("frame_n_path"),
            "previous should now hold frame N's paths");
        assert!(!sm.previous_active_paths.contains("stale"),
            "previous's prior contents should be discarded");
    }

    #[test]
    fn rotate_active_paths_preserves_diff_across_renders() {
        let mut sm = StateManager::new();
        sm.active_state_paths.insert("a".to_string());
        sm.active_state_paths.insert("b".to_string());

        sm.rotate_active_paths();
        // simulate frame N+1: only "a" gets visited
        sm.active_state_paths.insert("a".to_string());

        // mounted = current − previous = {} (a was already active)
        let mounted: HashSet<&String> = sm.active_state_paths
            .difference(&sm.previous_active_paths).collect();
        assert!(mounted.is_empty(), "no newly-mounted paths");

        // unmounted = previous − current = {"b"}
        let unmounted: HashSet<&String> = sm.previous_active_paths
            .difference(&sm.active_state_paths).collect();
        assert_eq!(unmounted.len(), 1);
        assert!(unmounted.contains(&"b".to_string()));
    }

    #[test]
    fn lifecycle_active_false_for_module_without_hooks() {
        let proto = proto_with_op(OpCode::Void);
        assert!(!Runtime::bytecode_uses_lifecycle(&proto));
    }

    #[test]
    fn lifecycle_active_true_for_each_hook_opcode() {
        for op in [
            OpCode::RegisterMountHook(0),
            OpCode::RegisterUnmountHook(0),
            OpCode::RegisterEffect { hook_id: 0, dep_count: 0 },
            OpCode::RegisterEffectCleanup,
        ] {
            let proto = proto_with_op(op);
            assert!(Runtime::bytecode_uses_lifecycle(&proto),
                "opcode should mark module as lifecycle-active");
        }
    }

    #[test]
    fn lifecycle_active_recurses_into_sub_protos() {
        let inner = proto_with_op(OpCode::RegisterMountHook(0));
        let mut outer = proto_with_op(OpCode::Void);
        outer.protos.push(inner);
        assert!(Runtime::bytecode_uses_lifecycle(&outer),
            "scan should recurse into sub-protos");
    }

    #[test]
    fn lifecycle_active_resets_on_set_module() {
        let mut runtime = Runtime::new();
        runtime.lifecycle_active = true;
        // Use the public set_module which should clear the flag.
        let source = "let main = fn () { return 1; };";
        let mut scanner = Scanner::new(source.to_string());
        let mut parser = Parser::new(scanner.scan());
        let module = parser.parse().expect("parse");
        runtime.set_module(module);
        assert!(!runtime.lifecycle_active,
            "set_module should reset lifecycle_active to false");
    }

    #[test]
    fn flush_for_path_prefix_queues_unmount_hooks() {
        let mut sm = StateManager::new();
        sm.unmount_hooks.insert(
            ("panel".to_string(), 0),
            dummy_closure("h", vec!["panel".to_string()]),
        );
        sm.unmount_hooks.insert(
            ("other".to_string(), 0),
            dummy_closure("h", vec!["other".to_string()]),
        );

        sm.flush_for_path_prefix("panel");

        assert_eq!(sm.pending_unmounts.len(), 1);
        assert_eq!(sm.pending_unmounts[0].0, "panel");
        assert!(!sm.unmount_hooks.contains_key(&("panel".to_string(), 0)),
            "hook should be removed from persistent map");
        assert!(sm.unmount_hooks.contains_key(&("other".to_string(), 0)),
            "non-matching prefix should be untouched");
    }

    #[test]
    fn flush_for_path_prefix_removes_effect_slots() {
        let mut sm = StateManager::new();
        sm.effects.insert(
            ("panel".to_string(), 0),
            EffectSlot {
                previous_deps: None,
                pending_cleanup: Some(dummy_closure("c", vec!["panel".to_string()])),
                closure: dummy_closure("e", vec!["panel".to_string()]),
            },
        );
        sm.effects.insert(
            ("other".to_string(), 0),
            EffectSlot {
                previous_deps: None,
                pending_cleanup: None,
                closure: dummy_closure("e", vec!["other".to_string()]),
            },
        );

        sm.flush_for_path_prefix("panel");

        assert_eq!(sm.pending_effect_cleanups.len(), 1,
            "effect with pending_cleanup should be queued");
        assert!(!sm.effects.contains_key(&("panel".to_string(), 0)),
            "effect slot should be removed from persistent map");
        assert!(sm.effects.contains_key(&("other".to_string(), 0)),
            "non-matching prefix should be untouched");
    }

    #[test]
    fn flush_for_empty_prefix_is_noop() {
        let mut sm = StateManager::new();
        sm.unmount_hooks.insert(
            ("any".to_string(), 0),
            dummy_closure("h", vec!["any".to_string()]),
        );
        sm.flush_for_path_prefix("");
        assert_eq!(sm.unmount_hooks.len(), 1,
            "empty prefix must not match anything");
        assert!(sm.pending_unmounts.is_empty());
    }

    #[test]
    fn cancel_unmount_for_prefix_clears_candidates() {
        let mut sm = StateManager::new();
        sm.candidate_unmounts.insert("panel".to_string());
        sm.candidate_unmounts.insert("other".to_string());

        sm.cancel_unmount_for_prefix("panel");

        assert!(!sm.candidate_unmounts.contains("panel"));
        assert!(sm.candidate_unmounts.contains("other"));
    }

    #[test]
    fn pending_queues_initially_empty_in_new_state_manager() {
        let sm = StateManager::new();
        assert!(sm.pending_mounts.is_empty());
        assert!(sm.pending_unmounts.is_empty());
        assert!(sm.pending_effect_fires.is_empty());
        assert!(sm.pending_effect_cleanups.is_empty());
        assert!(sm.unmount_hooks.is_empty());
        assert!(sm.effects.is_empty());
        assert!(sm.previous_active_paths.is_empty());
        assert!(sm.candidate_unmounts.is_empty());
    }

    #[test]
    fn widget_descriptor_captures_owned_path_at_vm_time() {
        // Regression test: widget descriptors must capture
        // call_stack at the moment the Widget opcode runs in the
        // VM, not when the builder later materializes them. By
        // the time execute_module returns, call_stack is empty;
        // capturing in the builder would always yield "".
        //
        // We verify by running a module whose `panel` fn returns
        // a Flex; the resulting Value::Widget should carry an
        // owned_path naming `panel` (with the call-counter
        // suffix).
        let source = r#"
let panel = fn () { Flex { children: [] } };
let main  = fn () { panel() };
"#;
        let mut runtime = Runtime::from_source(source, None)
            .expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let result = runtime.execute_module(&module).expect("execute");

        let Value::Widget(descriptor) = result else {
            panic!("expected top-level Widget value, got {:?}", result);
        };
        // The exact format of the path identifier is an
        // implementation detail (anonymous `fn` bindings get a
        // synthetic "fn@N" name). The contract is just: when a
        // widget is produced from inside a function call, its
        // owned_path is non-empty. A widget produced at module
        // top level (no surrounding fn) would have owned_path = "".
        assert!(!descriptor.owned_path.is_empty(),
            "widget descriptor must capture a non-empty path \
             when produced from inside an fn call; got: {:?}",
            descriptor.owned_path);
    }

    #[test]
    fn call_opcode_skips_path_marking_when_lifecycle_inactive() {
        // Hookless module: lifecycle_active stays false; the Call
        // opcode should not insert paths into active_state_paths.
        let source = r#"
let inner = fn () { return 1; };
let main  = fn () { return inner(); };
"#;
        let mut runtime = Runtime::from_source(source, None)
            .expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let _ = runtime.execute_module(&module).expect("execute");

        assert!(!runtime.lifecycle_active,
            "module without lifecycle opcodes should leave flag false");
        // Note: without lifecycle_active, active_state_paths is only
        // populated by DeclareState — no `state` declarations here,
        // so it should be empty.
        assert!(runtime.state.active_state_paths.is_empty(),
            "Call opcode must not mark paths when lifecycle_active=false");
    }

    // Suppress dead-code warnings for the synthetic helpers when
    // future merges rearrange the test set.
    #[allow(dead_code)]
    fn _silence_unused_warnings(_: RefCell<()>) {}
}
