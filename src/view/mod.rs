//! # The view model: applications, views, and instances
//!
//! This is the composition/embedding layer that sits *above* the single
//! `Instance` boundary — the layer [`crate`]'s `INTENT.md` §1–§9 leave
//! unspecified. See `docs/internal/VIEW_MODEL.md` for the full contract.
//!
//! ```text
//! application            root scope: always-live shell state + host services
//!   └─ view              recursive scope: a state slice + (a leaf OR child views)
//!        └─ instance     the live rendering of ONE .ogh source (the engine)
//!             └─ widgets the reconciled tree inside an instance
//! ```
//!
//! - [`Application`] — the singleton root; owns the shell [`Scope`] and the
//!   top-level [`ChildStack`].
//! - [`View`] — the recursive unit of isolation: a leaf XOR a branch
//!   (Tenet 3).
//! - `Instance` — the live rendering of one `.ogh` source. This is the type
//!   called [`crate::Ogham`] today; the view layer refers to it as
//!   `Instance` (the global struct rename is a later mechanical pass).
//!
//! The reconciler ([`ChildStack`]) is reconcile-by-key one scale up from the
//! widget `Presence` (Tenet 9). It runs under one of two policies
//! ([`StackPolicy`]): `Strict` reproduces the host-side exit-then-mount
//! single-occupancy of UL's `OghamPresence` (fullscreen screen swaps);
//! `Layered` gives coexisting z-ordered overlays (HUD + modal).

use std::collections::HashMap;
use std::hash::Hash;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::runtime::config::RuntimeConfig;
use crate::runtime::schema::{load_schema, load_schema_from_source, ModuleSchema, TypeRef};
use crate::runtime::value::Value;
// The engine instance. Renamed to `Instance` in the view layer per the
// VIEW_MODEL contract; the crate-wide struct rename is deferred to a single
// atomic pass, so we alias locally for now.
use crate::Ogham as Instance;

// ── scope + the chain assembled during traversal (Tenets 4–6) ─────────────

/// A view's typed state slice. Host-written (Rust side); read-only to `.ogh`
/// (INTENT §2). For now a loose `name → Value` map; the typed-provider match
/// (Tenet 6) is layered on in step 2.
#[derive(Default)]
pub struct Scope {
    state: HashMap<String, Value>,
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    /// The host provides a value into this scope (Tenet 6: a provider
    /// populates a scope; descendants resolve it up the chain).
    pub fn provide(&mut self, name: impl Into<String>, value: Value) {
        self.state.insert(name.into(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.state.get(name)
    }
}

/// The scope chain, **borrowed and assembled during the synchronous tick
/// traversal** — never stored on a view (Tenet 5). Resolution is leaf→root;
/// the nearest provider shadows farther ones.
pub struct ScopeChain<'a> {
    scope: &'a Scope,
    parent: Option<&'a ScopeChain<'a>>,
}

impl<'a> ScopeChain<'a> {
    /// Root the chain at the application/shell scope (Tenet 5).
    pub fn root(shell: &'a Scope) -> Self {
        Self {
            scope: shell,
            parent: None,
        }
    }

    /// Push a view's own scope onto the chain for the duration of its subtree
    /// traversal.
    pub fn push(&'a self, scope: &'a Scope) -> ScopeChain<'a> {
        ScopeChain {
            scope,
            parent: Some(self),
        }
    }

    /// Resolve a requirement leaf→root; nearest provider wins (Tenet 5).
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.scope
            .get(name)
            .or_else(|| self.parent.and_then(|p| p.get(name)))
    }
}

// ── per-child contract surfaces (Tenets 3, 9, 10) ─────────────────────────

/// The transition state machine for a child within a [`ChildStack`].
/// `Failed` = an unresolved requirement (Tenet 6) or an instance runtime
/// fault (Tenet 10) — the *same* event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Just minted. Under [`StackPolicy::Strict`] a `Mounting` child is the
    /// *frozen pending* destination — not ticked, rendered, or input-routed
    /// until the outgoing child's exit settles. Under `Layered` it is live
    /// from the first tick (it animates in).
    Mounting,
    Live,
    /// Caught failure (Tenet 10); propagates to the nearest `Fallback`.
    Failed,
    Exiting,
}

/// `Fallback` children are shown when a sibling subtree `Failed` (Tenet 10).
/// A branch that owns a `Fallback` child *is* an error boundary — there is no
/// separate `ViewBody` variant for boundaries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Normal,
    Fallback,
}

/// How a [`ChildStack`] reconciles its children.
///
/// Two policies of *one* reconciler (Tenet 9): `Strict` realizes
/// "replace-at-slot → strict swap", `Layered` realizes "depart →
/// exit-then-drop" for coexisting layers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackPolicy {
    /// Single-occupancy, exit-then-mount. At most one visible child; a new
    /// desired key waits (as a frozen `Mounting` pending) for the outgoing
    /// child's exit to settle before it mounts. Reproduces UL's
    /// `OghamPresence`.
    Strict,
    /// Coexisting z-ordered layers. Arrivals mount immediately and animate in
    /// alongside existing layers; departures exit-then-drop in place.
    Layered,
}

// ── view ──────────────────────────────────────────────────────────────────

/// One declared `host_state` field a leaf requires to be reachable in scope
/// (Tenet 6). Resolved leaf→root up the chain at tick time.
struct Requirement {
    name: String,
    /// `T?` — resolves to `Value::Void` when no provider is found, instead of
    /// failing (Tenet 6 / Tenet 10: the one explicit "absence is acceptable").
    optional: bool,
}

/// Translate a module schema's `host_state` block into requirements. Empty for
/// a loose module (no `host_state {}`) — such a leaf pulls nothing from scope
/// and never fails on a missing provider.
fn requirements_from_schema(schema: &ModuleSchema) -> Vec<Requirement> {
    match &schema.host_state {
        Some(hs) => hs
            .fields
            .iter()
            .map(|(name, field)| Requirement {
                name: name.clone(),
                optional: matches!(field.ty, TypeRef::Optional(_)),
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Read requirements off an already-built instance's compiled module.
fn extract_requirements(instance: &Instance) -> Vec<Requirement> {
    instance.with_runtime_mut(|rt| {
        rt.get_module()
            .and_then(|m| ModuleSchema::from_module(m).ok())
            .map(|schema| requirements_from_schema(&schema))
            .unwrap_or_default()
    })
}

/// A recursive scope and unit of isolation: a leaf XOR a branch (Tenet 3).
pub struct View<K> {
    key: K,
    scope: Scope,
    body: ViewBody<K>,
    /// A leaf's `host_state` requirements (Tenet 6); empty for a branch or a
    /// loose leaf. Computed once at construction.
    requires: Vec<Requirement>,
}

enum ViewBody<K> {
    /// Renders one source = one layer. Boxed: an `Instance` is large
    /// (~840B) and most views are leaves, so keep `Child`/the stack vec slim.
    Leaf(LeafState),
    /// Orders child views; renders nothing itself.
    Branch(ChildStack<K>),
}

/// A leaf is either already built (eager) or a deferred spec that the
/// framework constructs at *mount time* — the first tick, when the scope
/// chain is available (Tenet 6: "verified when the view mounts"). Deferring
/// construction lets the framework resolve `host_state` requirements and seed
/// them into the runtime *before* the module's first execution, so a module
/// that reads a required field never executes against an unprovided scope.
enum LeafState {
    /// Not yet constructed. Built (resolved + seeded + executed) at first tick.
    /// Boxed: a `LeafSpec` carries a full `RuntimeConfig` (~296B), kept off the
    /// common `Mounted` path.
    Pending(Box<LeafSpec>),
    Mounted(Box<Instance>),
}

/// How a deferred leaf is built once its requirements resolve.
struct LeafSpec {
    source: LeafSource,
    config: RuntimeConfig,
}

enum LeafSource {
    /// Inline `.ogh` source.
    Source(String),
    /// A watched file path (hot reload via the instance's own watcher).
    Path(String),
}

impl<K: Clone + Eq + Hash> View<K> {
    /// A leaf view over an already-built [`Instance`] (eager). The instance's
    /// `host_state` requirements are read from its compiled module. Use this
    /// when the host has constructed (and configured) the instance itself;
    /// prefer [`Self::leaf_pending`] when a leaf reads required host_state, so
    /// the framework can seed it before first execution.
    pub fn leaf(key: K, instance: Instance) -> Self {
        let requires = extract_requirements(&instance);
        Self {
            key,
            scope: Scope::new(),
            body: ViewBody::Leaf(LeafState::Mounted(Box::new(instance))),
            requires,
        }
    }

    /// A **host-managed** mounted leaf: the view layer sequences its lifecycle
    /// (transitions, entry/exit animation, render-order) but does NOT resolve
    /// or inject any `host_state` from the scope chain. The host drives the
    /// instance's `host_state` and rendering out-of-band (e.g. a controller
    /// calling [`Instance::tick`] each frame). The view tick therefore only
    /// advances animations — no scope resolution, no clobbering of
    /// host-injected state.
    ///
    /// Use this (not [`Self::leaf`]) for instances whose `host_state` comes
    /// from the embedder rather than the composition scope. `requires` is
    /// empty, so a declared-but-host-fed `host_state` field never trips the
    /// missing-required failure path.
    pub fn leaf_host_managed(key: K, instance: Instance) -> Self {
        Self {
            key,
            scope: Scope::new(),
            body: ViewBody::Leaf(LeafState::Mounted(Box::new(instance))),
            requires: Vec::new(),
        }
    }

    /// A deferred leaf from inline `.ogh` source. Construction is postponed to
    /// mount time (first tick): the framework resolves the declared
    /// `host_state` requirements against the scope chain, seeds them into the
    /// runtime config, then executes. A missing *required* provider fails the
    /// leaf at mount (Tenet 10) — the module never executes against an
    /// unprovided scope, so it cannot crash on an unset field.
    pub fn leaf_pending(key: K, source: impl Into<String>, config: RuntimeConfig) -> Self {
        let source = source.into();
        let requires = load_schema_from_source(&source)
            .map(|s| requirements_from_schema(&s))
            .unwrap_or_default();
        Self {
            key,
            scope: Scope::new(),
            body: ViewBody::Leaf(LeafState::Pending(Box::new(LeafSpec {
                source: LeafSource::Source(source),
                config,
            }))),
            requires,
        }
    }

    /// A deferred leaf from a watched file path (hot reload). Otherwise as
    /// [`Self::leaf_pending`].
    pub fn leaf_watch_pending(key: K, path: impl Into<String>, config: RuntimeConfig) -> Self {
        let path = path.into();
        let requires = load_schema(std::path::Path::new(&path))
            .map(|s| requirements_from_schema(&s))
            .unwrap_or_default();
        Self {
            key,
            scope: Scope::new(),
            body: ViewBody::Leaf(LeafState::Pending(Box::new(LeafSpec {
                source: LeafSource::Path(path),
                config,
            }))),
            requires,
        }
    }

    /// A branch view composing child views under `policy`.
    pub fn branch(key: K, policy: StackPolicy) -> Self {
        Self {
            key,
            scope: Scope::new(),
            body: ViewBody::Branch(ChildStack::new(policy)),
            requires: Vec::new(),
        }
    }

    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn scope_mut(&mut self) -> &mut Scope {
        &mut self.scope
    }

    /// Mutable access to a branch's child stack (host drives nested
    /// reconciliation through here). `None` for a leaf.
    pub fn children_mut(&mut self) -> Option<&mut ChildStack<K>> {
        match &mut self.body {
            ViewBody::Branch(cs) => Some(cs),
            ViewBody::Leaf(_) => None,
        }
    }

    /// A branch's child stack. `None` for a leaf.
    pub fn children(&self) -> Option<&ChildStack<K>> {
        match &self.body {
            ViewBody::Branch(cs) => Some(cs),
            ViewBody::Leaf(_) => None,
        }
    }

    pub fn instance(&self) -> Option<&Instance> {
        match &self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => Some(&**i),
            ViewBody::Leaf(LeafState::Pending(_)) | ViewBody::Branch(_) => None,
        }
    }

    pub fn instance_mut(&mut self) -> Option<&mut Instance> {
        match &mut self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => Some(&mut **i),
            ViewBody::Leaf(LeafState::Pending(_)) | ViewBody::Branch(_) => None,
        }
    }

    // ── recursive lifecycle (leaf delegates to Instance; branch recurses) ──
    // A still-`Pending` leaf has nothing built yet: no exit to play, exit is
    // trivially complete, entry/drain are no-ops.

    /// Begin exiting this view's subtree. Returns `true` if anything has an
    /// exit animation in flight (the caller keeps ticking and polls
    /// [`Self::is_exit_complete`]); `false` if there is nothing to animate.
    fn begin_exit(&mut self) -> bool {
        match &mut self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => {
                i.cancel_active_drag();
                i.begin_exit_root()
            }
            ViewBody::Leaf(LeafState::Pending(_)) => false,
            ViewBody::Branch(cs) => cs.begin_exit_all(),
        }
    }

    fn cancel_exit(&mut self) {
        match &mut self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => i.cancel_exit_root(),
            ViewBody::Leaf(LeafState::Pending(_)) => {}
            ViewBody::Branch(cs) => cs.cancel_exit_all(),
        }
    }

    fn is_exit_complete(&self) -> bool {
        match &self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => i.is_exit_complete_root(),
            ViewBody::Leaf(LeafState::Pending(_)) => true,
            ViewBody::Branch(cs) => cs.is_exit_complete_all(),
        }
    }

    fn restart_entry(&mut self) {
        match &mut self.body {
            ViewBody::Leaf(LeafState::Mounted(i)) => i.restart_entry_animations(),
            ViewBody::Leaf(LeafState::Pending(_)) => {}
            ViewBody::Branch(cs) => cs.restart_entry_all(),
        }
    }

    /// Tick this view's subtree by `dt` under `chain` (the scope context this
    /// view lives in). The view extends the chain with its own scope, then:
    ///
    /// - a **leaf** resolves its `host_state` requirements against the chain
    ///   (Tenets 5–6), injects the resolved values diff-aware, re-renders, and
    ///   ticks animations — all under per-instance fault isolation (Tenet 10);
    /// - a **branch** ticks its child stack under the extended chain.
    ///
    /// Returns `true` if the subtree faulted (Tenet 10): a missing *required*
    /// provider, an instance that panicked or errored during render, or a
    /// branch whose failure escaped its own boundary.
    fn tick(&mut self, dt: f32, chain: &ScopeChain) -> bool {
        // Disjoint field borrows: own scope (read) + body (write) + requires.
        let View {
            scope,
            body,
            requires,
            ..
        } = self;
        let chain = chain.push(scope);
        match body {
            ViewBody::Leaf(state) => tick_leaf(state, requires, dt, &chain),
            ViewBody::Branch(cs) => cs.tick_chained(dt, &chain),
        }
    }
}

/// Resolve a leaf's requirements against `chain`, returning the values to
/// inject (or seed). `Err(())` is a caught failure (Tenet 10): a *required*
/// provider was unresolved. A missing *optional* resolves to `Value::Void`.
fn resolve_requirements(
    requires: &[Requirement],
    chain: &ScopeChain,
) -> Result<Vec<(String, Value)>, ()> {
    let mut out = Vec::with_capacity(requires.len());
    for req in requires {
        match chain.get(&req.name) {
            Some(v) => out.push((req.name.clone(), v.clone())),
            None if req.optional => out.push((req.name.clone(), Value::Void)),
            None => return Err(()), // unresolved required → Failed
        }
    }
    Ok(out)
}

/// Tick a leaf under `chain`. Returns `true` if the leaf faulted (Tenet 10).
///
/// A `Pending` leaf resolves its requirements, seeds them into the config, and
/// constructs at this first tick (mount time) — so the module's first
/// execution always sees a fully-provided scope. A `Mounted` leaf re-resolves
/// and injects diff-aware, re-renders, and animates. All instance work is
/// fault-isolated: a panic or render error fails this leaf alone.
fn tick_leaf(state: &mut LeafState, requires: &[Requirement], dt: f32, chain: &ScopeChain) -> bool {
    match state {
        LeafState::Pending(spec) => {
            let seeds = match resolve_requirements(requires, chain) {
                Ok(s) => s,
                Err(()) => return true, // missing required at mount → Failed
            };
            // Seed resolved values into the config, then build. A construction
            // panic or error fails the leaf (never reaches the host).
            let built = catch_unwind(AssertUnwindSafe(|| build_leaf(spec, seeds)));
            match built {
                Ok(Ok(mut inst)) => {
                    catch_unwind(AssertUnwindSafe(|| inst.get_ui_mut().tick_animations(dt))).ok();
                    *state = LeafState::Mounted(Box::new(inst));
                    false
                }
                _ => true, // construct error or panic → Failed
            }
        }
        LeafState::Mounted(inst) => {
            // A loose leaf (no host_state) pulls nothing from scope — keep the
            // pure animate path so it never touches the runtime.
            if requires.is_empty() {
                return catch_unwind(AssertUnwindSafe(|| {
                    inst.get_ui_mut().tick_animations(dt);
                }))
                .is_err();
            }
            let injections = match resolve_requirements(requires, chain) {
                Ok(i) => i,
                Err(()) => return true, // a provider disappeared → Failed
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                inst.with_runtime_mut(|rt| rt.inject_host_state_batch(injections));
                let rendered_ok = inst.update().is_ok();
                inst.get_ui_mut().tick_animations(dt);
                rendered_ok
            }));
            match outcome {
                Ok(true) => false,
                Ok(false) => true, // render error → Failed
                Err(_) => true,    // panic → Failed
            }
        }
    }
}

/// Build a deferred leaf's instance, seeding resolved requirements into the
/// runtime config so the module's first execution sees them.
fn build_leaf(
    spec: &LeafSpec,
    seeds: Vec<(String, Value)>,
) -> Result<Instance, crate::runtime::error::RuntimeError> {
    let mut config = spec.config.clone();
    let mut host_state = config.host_state.take().unwrap_or_default();
    host_state.extend(seeds);
    config = config.with_host_state(host_state);
    match &spec.source {
        LeafSource::Source(src) => Instance::from_source(src, config),
        LeafSource::Path(path) => Instance::watch(path.clone(), config),
    }
}

// ── the recursive child manager: reconcile-by-key one scale up (Tenet 9) ──

struct Child<K> {
    view: View<K>,
    phase: Phase,
    role: Role,
}

impl<K: Clone + Eq + Hash> Child<K> {
    fn key(&self) -> &K {
        self.view.key()
    }

    /// Whether this child is painted / hit-tested this frame. A frozen
    /// `Strict` pending and a dormant `Fallback` (both `Mounting`) are
    /// neither.
    fn is_visible(&self, policy: StackPolicy) -> bool {
        match self.phase {
            Phase::Live | Phase::Exiting => true,
            Phase::Failed => false,
            // A layered arrival animates in from its first frame; a frozen
            // strict pending and a dormant fallback do not show.
            Phase::Mounting => policy == StackPolicy::Layered && self.role == Role::Normal,
        }
    }
}

/// An ordered (bottom→top) stack of child views reconciled by key.
///
/// This is the same transition machine as the widget `Presence`, one scale up
/// (Tenet 9): arrive → mount, depart → exit-then-drop, revert-mid-exit →
/// cancel-and-unwind, rapid change → latest-wins. Failures (Tenet 10)
/// propagate to the nearest `Fallback`-role child.
pub struct ChildStack<K> {
    children: Vec<Child<K>>,
    policy: StackPolicy,
}

impl<K: Clone + Eq + Hash> ChildStack<K> {
    pub fn new(policy: StackPolicy) -> Self {
        Self {
            children: Vec::new(),
            policy,
        }
    }

    pub fn policy(&self) -> StackPolicy {
        self.policy
    }

    /// Keys currently in the stack, in z-order (bottom→top), including those
    /// mid-exit and frozen pendings.
    pub fn keys(&self) -> Vec<K> {
        self.children.iter().map(|c| c.key().clone()).collect()
    }

    /// Keys to paint this frame, in z-order (bottom→top). Excludes a frozen
    /// `Strict` pending (Tenet 9 / `render_list_is_only_current`).
    pub fn render_order(&self) -> Vec<K> {
        self.children
            .iter()
            .filter(|c| c.is_visible(self.policy))
            .map(|c| c.key().clone())
            .collect()
    }

    /// `true` while a transition is in flight (a frozen pending exists, or any
    /// child is exiting). Mirrors `OghamPresence::is_transitioning` for the
    /// `Strict` case.
    pub fn is_transitioning(&self) -> bool {
        // Only *normal* children count: a dormant fallback also sits in
        // `Mounting`, but it is not a transition in flight (Tenet 10).
        self.children.iter().any(|c| {
            c.role == Role::Normal
                && (c.phase == Phase::Exiting || (self.is_strict() && c.phase == Phase::Mounting))
        })
    }

    fn is_strict(&self) -> bool {
        self.policy == StackPolicy::Strict
    }

    fn index_of(&self, key: &K) -> Option<usize> {
        self.children.iter().position(|c| c.key() == key)
    }

    fn push_child(&mut self, view: View<K>, phase: Phase, role: Role) {
        self.children.push(Child { view, phase, role });
    }

    /// Reconcile the stack against the host's `desired` set, minting any
    /// absent view via `mint`. The host decides which views exist; the stack
    /// owns their lifecycle (Non-goal: the stack does not decide membership).
    pub fn reconcile(&mut self, desired: &[K], mint: impl FnMut(&K) -> View<K>) {
        match self.policy {
            StackPolicy::Strict => self.reconcile_strict(desired, mint),
            StackPolicy::Layered => self.reconcile_layered(desired, mint),
        }
    }

    // ── Strict: single-occupancy, exit-then-mount (= OghamPresence) ────────

    /// In `Strict` mode the *last* desired key is the sole target (a stack is
    /// one logical slot; multi-slot is the deferred "slot" refinement). The
    /// children vec holds at most the current child plus one frozen pending.
    fn reconcile_strict(&mut self, desired: &[K], mut mint: impl FnMut(&K) -> View<K>) {
        // The current child is the visible normal child (Live or Exiting); the
        // pending is a frozen, never-yet-mounted normal child. Both finders
        // skip a dormant fallback, which also sits in `Mounting` (Tenet 10).
        let current = self
            .children
            .iter()
            .position(|c| c.role == Role::Normal && c.phase != Phase::Mounting);
        let pending = self
            .children
            .iter()
            .position(|c| c.role == Role::Normal && c.phase == Phase::Mounting);

        let target = match desired.last().cloned() {
            Some(t) => t,
            None => {
                // Nothing desired: drop the frozen pending, then exit current.
                self.drop_normal_pending();
                if let Some(ci) = self
                    .children
                    .iter()
                    .position(|c| c.role == Role::Normal && c.phase != Phase::Mounting)
                {
                    if self.children[ci].phase != Phase::Exiting {
                        if self.children[ci].view.begin_exit() {
                            self.children[ci].phase = Phase::Exiting;
                        } else {
                            self.children.remove(ci);
                        }
                    }
                }
                return;
            }
        };

        // Target already the current child.
        if let Some(ci) = current {
            if self.children[ci].key() == &target {
                // Revert: drop any frozen pending, cancel the current's exit.
                if pending.is_some() {
                    self.drop_normal_pending();
                    let ci = self
                        .children
                        .iter()
                        .position(|c| c.key() == &target)
                        .unwrap();
                    self.children[ci].view.cancel_exit();
                    self.children[ci].phase = Phase::Live;
                }
                return;
            }
        }

        // A pending is in flight.
        if let Some(pi) = pending {
            if self.children[pi].key() == &target {
                return; // already heading there
            }
            // Latest-wins: drop the abandoned pending and mint the new one.
            // The current child is *already exiting* — do not restart its exit.
            self.drop_normal_pending();
            self.push_child(mint(&target), Phase::Mounting, Role::Normal);
            return;
        }

        // Fresh transition: no pending in flight.
        match current {
            Some(ci) => {
                if self.children[ci].view.begin_exit() {
                    self.children[ci].phase = Phase::Exiting;
                    self.push_child(mint(&target), Phase::Mounting, Role::Normal);
                } else {
                    // No exit to play: swap immediately, preserving "one at a
                    // time, never both".
                    self.children.remove(ci);
                    self.swap_in_now(mint(&target));
                }
            }
            // Empty (only a dormant fallback, if any): mount directly.
            None => self.swap_in_now(mint(&target)),
        }
    }

    /// Drop the frozen, never-mounted normal pending (if any). Leaves a
    /// dormant fallback — also `Mounting` — untouched.
    fn drop_normal_pending(&mut self) {
        self.children
            .retain(|c| !(c.role == Role::Normal && c.phase == Phase::Mounting));
    }

    /// Promote a view to the visible current child immediately, restarting its
    /// entry animations.
    fn swap_in_now(&mut self, mut view: View<K>) {
        view.restart_entry();
        self.push_child(view, Phase::Live, Role::Normal);
    }

    // ── Layered: coexisting z-ordered overlays ─────────────────────────────

    fn reconcile_layered(&mut self, desired: &[K], mut mint: impl FnMut(&K) -> View<K>) {
        // Depart: any present *normal* child whose key is not desired exits.
        // A departure with no exit animation is dropped immediately (mirroring
        // the strict immediate-swap path) rather than parked in `Exiting`,
        // which would never settle since no animation is in flight.
        let mut drop_now = Vec::new();
        for i in 0..self.children.len() {
            if self.children[i].role != Role::Normal {
                continue; // a dormant fallback is not subject to the desired set
            }
            let present = self.children[i].key().clone();
            if !desired.contains(&present) && self.children[i].phase != Phase::Exiting {
                if self.children[i].view.begin_exit() {
                    self.children[i].phase = Phase::Exiting;
                } else {
                    drop_now.push(i);
                }
            }
        }
        for i in drop_now.into_iter().rev() {
            self.children.remove(i);
        }

        // Arrive / revert, in desired z-order.
        for key in desired {
            match self.index_of(key) {
                Some(i) => {
                    if self.children[i].phase == Phase::Exiting {
                        // Revert mid-exit: cancel and unwind back to live.
                        self.children[i].view.cancel_exit();
                        self.children[i].phase = Phase::Live;
                    }
                }
                None => {
                    let mut view = mint(key);
                    view.restart_entry();
                    self.push_child(view, Phase::Mounting, Role::Normal);
                }
            }
        }

        self.reorder_layered(desired);
    }

    /// Order children by `desired` z-order; exiting children not in `desired`
    /// keep their place on top (a closing overlay fades in place above what
    /// remains).
    fn reorder_layered(&mut self, desired: &[K]) {
        let rank = |key: &K| -> usize {
            desired.iter().position(|d| d == key).unwrap_or(usize::MAX) // exiting-not-desired sort last (top)
        };
        self.children.sort_by_key(|c| rank(c.key()));
    }

    // ── tick (drives phase transitions + fault propagation) ────────────────

    /// One frame. Ticks live children, promotes a `Strict` pending once the
    /// outgoing child's exit settles, drops settled exits, and propagates any
    /// `Failed` child to the nearest `Fallback` (Tenet 10).
    ///
    /// Returns `true` if a failure escaped this stack (no `Fallback` here to
    /// catch it) — the caller propagates further up.
    pub fn tick(&mut self, dt: f32) -> bool {
        // A standalone stack (no enclosing application) ticks under an empty
        // root scope: leaves that declare no host_state resolve trivially.
        let empty = Scope::new();
        let chain = ScopeChain::root(&empty);
        self.tick_chained(dt, &chain)
    }

    /// Tick under an explicit scope chain (threaded from the enclosing view /
    /// application). Each child view extends the chain with its own scope.
    pub fn tick_chained(&mut self, dt: f32, chain: &ScopeChain) -> bool {
        match self.policy {
            StackPolicy::Strict => self.tick_strict(dt, chain),
            StackPolicy::Layered => self.tick_layered(dt, chain),
        }
        self.resolve_failures()
    }

    fn tick_strict(&mut self, dt: f32, chain: &ScopeChain) {
        // Tick the visible current child: a normal current (Live or Exiting),
        // or an active fallback (Live). A frozen pending and a dormant
        // fallback (both Mounting) are skipped.
        let ci = match self
            .children
            .iter()
            .position(|c| matches!(c.phase, Phase::Live | Phase::Exiting))
        {
            Some(ci) => ci,
            None => return,
        };
        if dt > 0.0 && self.children[ci].view.tick(dt, chain) {
            self.children[ci].phase = Phase::Failed;
        }
        // Promote the frozen *normal* pending once the outgoing exit settles.
        if self.children[ci].phase == Phase::Exiting && self.children[ci].view.is_exit_complete() {
            self.children.remove(ci);
            if let Some(pi) = self
                .children
                .iter()
                .position(|c| c.role == Role::Normal && c.phase == Phase::Mounting)
            {
                self.children[pi].view.restart_entry();
                self.children[pi].phase = Phase::Live;
                // Tick the just-promoted child this same frame so a deferred
                // (Pending) leaf constructs now rather than rendering one blank
                // frame; a construction fault surfaces immediately.
                if dt > 0.0 && self.children[pi].view.tick(dt, chain) {
                    self.children[pi].phase = Phase::Failed;
                }
            }
        }
    }

    fn tick_layered(&mut self, dt: f32, chain: &ScopeChain) {
        let mut drop_idx = Vec::new();
        for i in 0..self.children.len() {
            // Don't tick a caught failure (it awaits boundary resolution or
            // drop; re-ticking could re-panic) or a dormant fallback (a
            // Mounting Fallback is activated only by `resolve_failures`, never
            // ticked into life — which also keeps a Pending fallback unbuilt
            // until it is actually needed).
            let skip = self.children[i].phase == Phase::Failed
                || (self.children[i].phase == Phase::Mounting
                    && self.children[i].role == Role::Fallback);
            if !skip && dt > 0.0 && self.children[i].view.tick(dt, chain) {
                self.children[i].phase = Phase::Failed;
            }
            // A freshly-mounted *normal* layer goes live after its first tick.
            // A dormant fallback stays Mounting until a failure activates it.
            if self.children[i].phase == Phase::Mounting && self.children[i].role == Role::Normal {
                self.children[i].phase = Phase::Live;
            }
            if self.children[i].phase == Phase::Exiting && self.children[i].view.is_exit_complete()
            {
                drop_idx.push(i);
            }
        }
        for i in drop_idx.into_iter().rev() {
            self.children.remove(i);
        }
    }

    /// Tenet 10: a `Failed` child is swapped for the nearest `Fallback`-role
    /// child in this stack. If there is no `Fallback` here, the failure
    /// escapes upward (return `true`); a parent boundary — ultimately
    /// `Application`'s root fallback — catches it.
    fn resolve_failures(&mut self) -> bool {
        let has_failed = self
            .children
            .iter()
            .any(|c| c.phase == Phase::Failed && c.role == Role::Normal);
        if !has_failed {
            return false;
        }
        if self.children.iter().any(|c| c.role == Role::Fallback) {
            // Drop the failed normal children; surface the fallback live.
            self.children
                .retain(|c| !(c.phase == Phase::Failed && c.role == Role::Normal));
            let fi = self
                .children
                .iter()
                .position(|c| c.role == Role::Fallback)
                .unwrap();
            if self.children[fi].phase == Phase::Mounting {
                self.children[fi].view.restart_entry();
                self.children[fi].phase = Phase::Live;
            }
            false
        } else {
            // No boundary here: the failure escapes to an ancestor (ultimately
            // Application's root fallback).
            true
        }
    }

    /// Install a fallback child (Tenet 10). A stack with a fallback *is* an
    /// error boundary.
    pub fn set_fallback(&mut self, view: View<K>) {
        // Replace any existing fallback.
        self.children.retain(|c| c.role != Role::Fallback);
        self.push_child(view, Phase::Mounting, Role::Fallback);
    }

    // ── recursive lifecycle helpers (called by a parent View) ──────────────

    fn begin_exit_all(&mut self) -> bool {
        let mut any = false;
        for c in &mut self.children {
            if c.view.begin_exit() {
                c.phase = Phase::Exiting;
                any = true;
            }
        }
        any
    }

    fn cancel_exit_all(&mut self) {
        for c in &mut self.children {
            if c.phase == Phase::Exiting {
                c.view.cancel_exit();
                c.phase = Phase::Live;
            }
        }
    }

    fn is_exit_complete_all(&self) -> bool {
        self.children.iter().all(|c| c.view.is_exit_complete())
    }

    fn restart_entry_all(&mut self) {
        for c in &mut self.children {
            c.view.restart_entry();
        }
    }

    // ── host-facing accessors (paint / input / state-provision) ───────────

    /// The child view for `key`, if present (including mid-exit / frozen).
    pub fn view(&self, key: &K) -> Option<&View<K>> {
        self.index_of(key).map(|i| &self.children[i].view)
    }

    /// Mutable child view for `key` — the host updates a provider scope or
    /// drives a nested branch's reconciliation through this.
    pub fn view_mut(&mut self, key: &K) -> Option<&mut View<K>> {
        match self.index_of(key) {
            Some(i) => Some(&mut self.children[i].view),
            None => None,
        }
    }

    /// The live engine instance for `key` (a constructed leaf), if any.
    pub fn instance(&self, key: &K) -> Option<&Instance> {
        self.index_of(key)
            .and_then(|i| self.children[i].view.instance())
    }

    /// Mutable engine instance for `key` — the host paints, lays out, and
    /// dispatches input through this. `None` for a branch, a still-`Pending`
    /// leaf, or an absent key.
    pub fn instance_mut(&mut self, key: &K) -> Option<&mut Instance> {
        match self.index_of(key) {
            Some(i) => self.children[i].view.instance_mut(),
            None => None,
        }
    }

    /// The visible "current" key for paint/layout: the topmost child that is
    /// painted this frame (a `Live` or `Exiting` normal child, or an active
    /// fallback), even mid-transition. Distinct from [`Self::input_target`],
    /// which gates input during a swap. `None` when nothing is visible.
    pub fn current(&self) -> Option<K> {
        // Top-most first (children are ordered bottom→top): in a layered stack
        // the upper layer is "current"; in a strict stack there is only one.
        // Prefer a live/exiting normal child; fall back to an active (Live)
        // fallback if a failure has surfaced one.
        self.children
            .iter()
            .rev()
            .find(|c| c.role == Role::Normal && matches!(c.phase, Phase::Live | Phase::Exiting))
            .or_else(|| {
                self.children
                    .iter()
                    .rev()
                    .find(|c| c.role == Role::Fallback && c.phase == Phase::Live)
            })
            .map(|c| c.key().clone())
    }

    /// The key that should receive input this frame, if any: the top-most
    /// `Live` normal child, when the stack is not mid-transition. `None` while
    /// transitioning (input is gated during a swap) or when only a fallback is
    /// live. Mirrors `OghamPresence::input_target`. (Per-layer `Modal` gating
    /// is a future refinement; today the top-most live layer wins.)
    pub fn input_target(&self) -> Option<K> {
        if self.is_transitioning() {
            return None;
        }
        self.children
            .iter()
            .rev()
            .find(|c| c.role == Role::Normal && c.phase == Phase::Live)
            .map(|c| c.key().clone())
    }

    // ── test introspection ────────────────────────────────────────────────

    #[cfg(test)]
    fn phase_of(&self, key: &K) -> Option<Phase> {
        self.index_of(key).map(|i| self.children[i].phase)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.children.len()
    }

    #[cfg(test)]
    fn instance_of(&self, key: &K) -> Option<&Instance> {
        self.instance(key)
    }

    /// Force a child into `Failed` to test fault propagation without injecting
    /// a real instance panic (Tenet 10).
    #[cfg(test)]
    fn force_fail(&mut self, key: &K) {
        if let Some(i) = self.index_of(key) {
            self.children[i].phase = Phase::Failed;
        }
    }

    #[cfg(test)]
    fn role_of(&self, key: &K) -> Option<Role> {
        self.index_of(key).map(|i| self.children[i].role)
    }

    #[cfg(test)]
    fn child_view(&self, key: &K) -> Option<&View<K>> {
        self.view(key)
    }

    #[cfg(test)]
    fn child_view_mut(&mut self, key: &K) -> Option<&mut View<K>> {
        self.view_mut(key)
    }
}

// ── application ─────────────────────────────────────────────────────────────

/// The singleton root: the always-live shell [`Scope`] plus the top-level
/// [`ChildStack`]. Not itself a `View` — it is the host's handle on the tree.
pub struct Application<K> {
    shell: Scope,
    top: ChildStack<K>,
}

impl<K: Clone + Eq + Hash> Application<K> {
    /// A new application. `top_policy` governs the top-level stack (UL's top
    /// level is `Strict` — fullscreen screens swap one at a time).
    pub fn new(top_policy: StackPolicy) -> Self {
        Self {
            shell: Scope::new(),
            top: ChildStack::new(top_policy),
        }
    }

    pub fn shell_mut(&mut self) -> &mut Scope {
        &mut self.shell
    }

    pub fn top_mut(&mut self) -> &mut ChildStack<K> {
        &mut self.top
    }

    /// Immutable view of the top-level stack — for read-only queries
    /// (`current`, `input_target`, `instance`) that don't need `&mut`.
    pub fn top(&self) -> &ChildStack<K> {
        &self.top
    }

    /// Install the application's root fallback — the default boundary that
    /// makes any uncaught failure *defined* behavior (Tenet 10).
    pub fn set_root_fallback(&mut self, view: View<K>) {
        self.top.set_fallback(view);
    }

    /// Reconcile the top stack against the host's desired set.
    pub fn reconcile_top(&mut self, desired: &[K], mint: impl FnMut(&K) -> View<K>) {
        self.top.reconcile(desired, mint);
    }

    /// One frame. The shell roots every read chain (Tenet 5); the top stack
    /// ticks under it. A failure that reaches here is bounded by the root
    /// fallback — it never reaches the host as a panic or a blank surface.
    pub fn tick(&mut self, dt: f32) {
        // The shell roots every read chain (Tenet 5). Disjoint borrows: shell
        // (read) and top (write) are separate fields.
        let chain = ScopeChain::root(&self.shell);
        let _escaped = self.top.tick_chained(dt, &chain);
        // `_escaped` can only be true if no root fallback was installed; with
        // the default boundary present it is always caught (Tenet 10).
    }

    /// Top-level keys to paint this frame, bottom→top.
    pub fn render_order(&self) -> Vec<K> {
        self.top.render_order()
    }
}

#[cfg(test)]
mod tests;
