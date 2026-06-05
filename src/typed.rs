//! Typed Ogham handle (Phase 1 / M5).
//!
//! [`TypedOgham<S, M>`] wraps an [`Ogham`] instance with a typed
//! state struct (`#[derive(OghamState)]`) and a typed message
//! enum (`#[derive(OghamMsg)]`). The constructor enforces a
//! startup schema-match check between the Rust types and the
//! parsed `.ogh` module schema; once that succeeds, every
//! per-frame interaction is type-checked at the boundary:
//!
//! - `set_state(&S)` diffs against the previous snapshot and
//!   pushes only changed fields (using
//!   `inject_host_state_if_changed` semantics).
//! - `poll_msg() -> Option<M>` drains typed messages from the
//!   internal MPSC queue that registered event handlers push
//!   into.
//!
//! See [`docs/internal/TYPED_BINDINGS.md`](../../docs/internal/TYPED_BINDINGS.md)
//! for the design contract and
//! [`docs/internal/TYPED_BINDINGS_IMPLEMENTATION.md`](../../docs/internal/TYPED_BINDINGS_IMPLEMENTATION.md)
//! for the implementation plan this code follows.

use std::marker::PhantomData;
use std::sync::mpsc::Receiver;

use crate::runtime::config::RuntimeConfig;
use crate::runtime::error::RuntimeError;
use crate::runtime::schema::{ModuleSchema, OghamMsg, OghamState};
use crate::runtime::value::Value;
use crate::Ogham;

/// Typed wrapper around an [`Ogham`] instance.
///
/// Construct via [`Ogham::watch_typed`] or
/// [`Ogham::from_source_typed`]. The two type parameters bind
/// the host state struct and the events enum at the type level
/// so callers can't accidentally pass mismatched values to a
/// schema that expects different shapes.
pub struct TypedOgham<S: OghamState, M: OghamMsg> {
    inner: Ogham,
    rx: Receiver<M>,
    last_state: S,
    _phantom: PhantomData<M>,
}

impl<S: OghamState + Clone, M: OghamMsg> TypedOgham<S, M> {
    /// Diff `new` against the last snapshot and inject only the
    /// fields that changed. No-op frames trigger no rerenders.
    pub fn set_state(&mut self, new: S) {
        let mut rt = self
            .inner
            .get_runtime()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        new.ogham_diff_apply(&self.last_state, &mut *rt);
        drop(rt);
        self.last_state = new;
    }

    /// Try to drain one message from the queue. Returns `None`
    /// when no messages are pending.
    pub fn poll_msg(&mut self) -> Option<M> {
        self.rx.try_recv().ok()
    }

    /// Drain every queued message. Useful when the consumer wants
    /// to apply all pending events in one batch (e.g. before
    /// rendering).
    pub fn drain_msgs(&mut self) -> impl Iterator<Item = M> + '_ {
        std::iter::from_fn(|| self.rx.try_recv().ok())
    }

    /// Borrow the underlying loose [`Ogham`] for layout, render,
    /// hot-reload, and any other API that doesn't need typing.
    pub fn inner(&self) -> &Ogham {
        &self.inner
    }

    /// Mutably borrow the underlying loose [`Ogham`].
    pub fn inner_mut(&mut self) -> &mut Ogham {
        &mut self.inner
    }

    /// Forward to [`Ogham::check_for_changes`] so callers don't
    /// have to reach through `inner()` for the common hot-reload
    /// poll path.
    pub fn check_for_changes(&self) -> bool {
        self.inner.check_for_changes()
    }

    /// Reload the watched file. Re-runs the schema-match check
    /// against `S` and `M` — schema-incompatible reloads return
    /// [`RuntimeError::SchemaMismatch`] and leave the existing
    /// runtime untouched. Schema-compatible reloads preserve the
    /// last typed state by pushing `self.last_state` into the
    /// freshly-reloaded runtime, so user-driven changes survive
    /// the reload.
    ///
    /// This intentionally fails loud on schema drift rather than
    /// silently coercing — see [`docs/internal/INTENT.md`](../../docs/internal/INTENT.md)
    /// §7 ("Hot reload preserves what it can, drops what it
    /// can't").
    pub fn reload(&mut self) -> Result<(), RuntimeError> {
        // Pull the watched path from the inner Ogham; if there
        // isn't one (constructed via from_source_typed), reload
        // is a no-op.
        let Some(path) = self.inner.get_path() else {
            return Ok(());
        };
        let path = path.to_string();

        // (1) Re-check the schema against the new file's shape.
        let parsed = load_schema_or_runtime_err(LoadSource::Path(&path))?;
        check_schemas_match::<S, M>(&parsed)?;

        // (2) Schema is compatible — reload the inner runtime.
        //     The inner.reload() path uses the stored config
        //     which still has the *initial* host_state. The
        //     module's first execution after reload sees that
        //     initial state.
        self.inner.reload()?;

        // (3) Overlay the latest typed state on top so any
        //     user-driven changes since construction are
        //     preserved across the reload.
        let runtime = self.inner.get_runtime().clone();
        let mut rt = runtime.lock().unwrap_or_else(|e| e.into_inner());
        self.last_state.ogham_snapshot_into(&mut *rt);
        Ok(())
    }
}

impl Ogham {
    /// Construct a typed Ogham instance with file watching.
    ///
    /// On success the parsed module's schema must match the
    /// derived `S` and `M` types exactly; mismatches return
    /// [`RuntimeError::SchemaMismatch`] with a diff describing
    /// each field/event that disagrees. `initial_state` is
    /// snapshotted into host_state before the first render.
    pub fn watch_typed<S, M>(
        path: String,
        initial_state: S,
        config: RuntimeConfig,
    ) -> Result<TypedOgham<S, M>, RuntimeError>
    where
        S: OghamState + Clone + 'static,
        M: OghamMsg,
    {
        let parsed_schema = load_schema_or_runtime_err(LoadSource::Path(&path))?;
        check_schemas_match::<S, M>(&parsed_schema)?;
        // Inject initial state *into the config's host_state map*
        // before constructing Ogham, so the first module execution
        // sees populated values. (Doing it after Ogham::watch
        // returns is too late — the module body has already run
        // and would have errored on `UndefinedVariable` for any
        // host_state field referenced at the top level.)
        let config = inject_initial_into_config::<S>(config, &initial_state);
        let (config, rx) = config.with_typed_events::<M>();
        let ogham = Ogham::watch(path, config)?;
        Ok(TypedOgham {
            inner: ogham,
            rx,
            last_state: initial_state,
            _phantom: PhantomData,
        })
    }

    /// Like [`watch_typed`](Self::watch_typed) but constructs
    /// from a source string (no file watching).
    pub fn from_source_typed<S, M>(
        source: &str,
        initial_state: S,
        config: RuntimeConfig,
    ) -> Result<TypedOgham<S, M>, RuntimeError>
    where
        S: OghamState + Clone + 'static,
        M: OghamMsg,
    {
        let parsed_schema = load_schema_or_runtime_err(LoadSource::Source(source))?;
        check_schemas_match::<S, M>(&parsed_schema)?;
        let config = inject_initial_into_config::<S>(config, &initial_state);
        let (config, rx) = config.with_typed_events::<M>();
        let ogham = Ogham::from_source(source, config)?;
        Ok(TypedOgham {
            inner: ogham,
            rx,
            last_state: initial_state,
            _phantom: PhantomData,
        })
    }
}

/// Validate that the `.ogh` file at `path` declares `host_state {}` / `events {}`
/// matching the typed `S` / `M` pair — the same startup check
/// [`Ogham::watch_typed`] runs — WITHOUT constructing a [`TypedOgham`]. Use this
/// from a *decoupled* controller's `new()` (where the `Ogham` is owned elsewhere,
/// e.g. a view-tree `Application`, and the host wires events via
/// [`crate::runtime::config::RuntimeConfig::with_typed_events`]) so `.ogh`↔Rust
/// schema drift fails loud at boot instead of degrading to dropped events.
pub fn check_schemas_match_path<S, M>(path: &str) -> Result<(), RuntimeError>
where
    S: OghamState,
    M: OghamMsg,
{
    let parsed = load_schema_or_runtime_err(LoadSource::Path(path))?;
    check_schemas_match::<S, M>(&parsed)
}

enum LoadSource<'a> {
    Path(&'a str),
    Source(&'a str),
}

fn load_schema_or_runtime_err(src: LoadSource<'_>) -> Result<ModuleSchema, RuntimeError> {
    let result = match src {
        LoadSource::Path(p) => crate::runtime::schema::load_schema(std::path::Path::new(p)),
        LoadSource::Source(s) => crate::runtime::schema::load_schema_from_source(s),
    };
    result.map_err(|e| match e {
        crate::runtime::schema::SchemaLoadError::Io(io) => RuntimeError::IoError(io),
        crate::runtime::schema::SchemaLoadError::Syntax(syn) => RuntimeError::SyntaxError(syn),
        crate::runtime::schema::SchemaLoadError::Scanner(msg) => {
            RuntimeError::SyntaxError(crate::parser::SyntaxError::new(0, 0, msg))
        }
    })
}

/// Take the runtime config and merge in initial state by
/// snapshotting `S` into a `HashMap<String, Value>` that flows
/// through `RuntimeConfig::with_host_state`. Necessary because
/// the runtime executes the module body during `Ogham::watch` /
/// `from_source`, and that execution reads host_state values —
/// so they must be present *before* the runtime is constructed,
/// not pushed in afterwards.
fn inject_initial_into_config<S: OghamState>(
    mut config: RuntimeConfig,
    initial: &S,
) -> RuntimeConfig {
    use std::collections::HashMap;
    let mut sink: HashMap<String, Value> = config.host_state.take().unwrap_or_default();
    initial.ogham_snapshot_into(&mut sink);
    config.with_host_state(sink)
}

/// Verify the parsed module's schema matches the Rust-side
/// derived schemas. Returns a `RuntimeError::SchemaMismatch` with a
/// diff string on disagreement.
///
/// As of P0-M4 this is a thin wrapper around the data-shaped
/// [`crate::diagnostics::check_against_manifest`] backend: we
/// synthesize a [`StateManifest`] / [`EventsManifest`] pair from
/// the generic types and run the same backend the static (CLI / LSP)
/// paths use. Keeps the runtime path's belt-and-suspenders behaviour
/// identical while sharing logic with the cross-side diagnostic
/// surface.
///
/// Records referenced by name don't need their own pass: if both
/// sides reference `Record("Player")`, they're considered equal.
fn check_schemas_match<S, M>(parsed: &ModuleSchema) -> Result<(), RuntimeError>
where
    S: OghamState,
    M: OghamMsg,
{
    let derived_hs = S::ogham_record_schema();
    // SchemaMissing only fires when the Rust state has fields
    // but the module declares no `host_state {}`. An empty Rust
    // state matched against an absent `host_state` is fine —
    // event-only modules (like chest_ui) live here.
    if parsed.host_state.is_none() && !derived_hs.fields.is_empty() {
        return Err(RuntimeError::SchemaMissing);
    }

    // Synthesize manifests from the generic types and run the same
    // backend the CLI/LSP front-ends use. `ogh_module` is empty in
    // this path: `check_against_manifest` never reads the field —
    // matching by .ogh module path is the LSP/CLI's job — so the
    // empty value is invisible at runtime. If anyone ever serializes
    // a runtime-synthesized manifest for debugging, that's the
    // caveat to be aware of.
    let state_manifest = crate::diagnostics::manifest::StateManifest::from_state::<S>("");
    let events_manifest = crate::diagnostics::manifest::EventsManifest::from_events::<M>("");

    let diags = crate::diagnostics::check_against_manifest(
        parsed,
        Some(&state_manifest),
        Some(&events_manifest),
        std::any::type_name::<S>(),
    );

    if diags.is_empty() {
        Ok(())
    } else {
        let rendered = diags
            .iter()
            .map(|d| format!("  - {}", d.message))
            .collect::<Vec<_>>()
            .join("\n");
        Err(RuntimeError::SchemaMismatch(rendered))
    }
}
