# Ogham — The View Model: Applications, Views, and Instances

> **Status: Proposed.**
>
> This document specifies Ogham's *composition and embedding* contract:
> how a host assembles many `.ogh` sources into one running UI, how
> state flows across them, and what the framework owns versus what the
> embedder owns.
>
> It is the layer [`INTENT.md`](INTENT.md) §1–§9 deliberately does not
> cover. Those tenets govern the inside of a *single* instance — the
> four-pass pipeline, reconciliation, hooks, the render seam — and all
> remain live and untouched. This document governs the structure *at
> and above* the instance boundary, which is where every real embedder
> (Untold Lore today, ~25 hand-juggled instances — 23 in a keyed
> overlay registry plus root-adjacent escapees) is forced to invent
> its own answers.
>
> It **supersedes [`STAGE.md`](STAGE.md)**, whose host-side "Stage"
> orchestrator dissolved into the `application` / `view` model below.
>
> Tenets follow the repo's rule + *Why* + *Drift indicators* shape.
> Genuinely unresolved details are collected under Open Questions; this
> doc describes the **end-state architecture**, not the path to it.

---

## The gap this fills

INTENT specifies Ogham exquisitely *below* the instance boundary and is
silent *at and above* it. There is no tenet for "how does a host hold
many instances," "what is the unit of isolation," "how does state reach
nested UI," or "who owns focus, input, the frame clock, and the surface
when there is more than one instance." The `Ogham` struct was therefore
overloaded: it was simultaneously a **unit** (one source's runtime +
tree) and a **root** (owner of the watcher, clock, input, and surface).
Those roles only fuse cleanly when there is exactly one instance. This
document splits them.

The overload is visible in Untold Lore: most instances live in the
keyed overlay registry, but the root-adjacent ones (`MenuState.ogham`,
`escape_ogham: Option<Ogham>`) leak out as hand-placed one-off fields on
the client, because the orchestrator has no concept of an instance that
*is* the root rather than an overlay within it. Splitting unit from root
gives those a home (a leaf in the application branch) instead of a
special case.

---

## The model at a glance

```
application            root scope: always-live shell state + host services
  └─ view              recursive scope: a state slice + (a leaf OR child views)
       └─ instance     the live rendering of ONE .ogh source (the engine)
            └─ widgets  the reconciled tree inside an instance
```

- **`application`** — the singleton root. Owns the always-live *shell
  scope* (theme, locale, viewport, DPI, fonts, input bindings) and the
  host *services* no single view should own: the surface, the frame
  clock, input arbitration, and the hot-reload watcher pool. It is not
  itself a `view`; it is the host's handle on the tree.
- **`view`** — the recursive unit of isolation and scope. Owns a typed
  *state slice* and is **either** a leaf (renders one `instance`) **or**
  a branch (composes child views) — never both. Views define the app's
  lifecycle hierarchy (`application → view<campaign> → view<map> →
  view<inventory>`); the framework supplies the recursion, the embedder
  supplies the tree.
- **`instance`** — the live rendering of one `.ogh` source: its runtime
  (VM + local state), its reconciled widget tree, and the source id for
  hot-reload routing. This is the type called `Ogham` today, **renamed**
  (struct only, not the project) to reflect its narrowed role. Every
  INTENT §1–§9 invariant lives here, unchanged.
- **widgets** — the reconciled tree inside an instance, governed by the
  existing widget tenets.

---

## Tenets

### 1. Three concepts: application, view, instance — at three scales.

`application` is the root scope and the owner of host services. `view`
is the recursive scope and unit of isolation. `instance` is the engine
that renders one source. Each layer has exactly one job; responsibility
does not leak between them.

| Responsibility | Owner |
|---|---|
| compiled program + VM + a source's local state | **instance** |
| reconciled widget tree (anim/hover/scroll/focus state) | **instance** |
| a lifecycle-scoped state slice; child composition | **view** |
| isolation boundary (own scope, own hot-reload unit) | **view** |
| always-live shell state (theme/locale/viewport/dpi/fonts) | **application** |
| host services: surface, frame clock, input, watcher pool | **application** |

**Why:** focus, input arbitration, frame scheduling, and compositing are
inherently *cross-instance* concerns. The single-instance `Ogham` had no
home for them, so every embedder reinvents them (Untold Lore's
`OghamPresence` + `ui.rs` are exactly this). Naming `application` as
their owner removes the embedder's obligation to be the root.

**Drift indicators:**
- An `instance` that owns a file watcher, a frame clock, or a surface.
- A `view` that reaches a sibling's state directly instead of through a
  shared ancestor scope.
- Shell concerns (theme, fonts) injected per-instance instead of read
  from the application scope.

### 2. Views wrap instances; the engine never becomes recursive.

A `view` *contains* an `instance` (in the leaf case). An `instance`
never contains another `instance`. All nesting, scoping, and lifecycle
live in the `view` layer, *above* the engine.

**Why:** INTENT §1–§9 are single-instance invariants — hook-path
identity (§9), host-state asymmetry (§2), reconcile-not-rebuild (§3),
the render seam (§6), hot reload (§7) all hold *because* there is one
runtime, one tree, one path space per instance. Making the engine
recursive would thread scope chains through the VM, the builder, and the
hook registry, putting all nine tenets at risk. Wrapping keeps the
proven core untouched and makes the entire composition model *additive*.
A view boundary is therefore also a clean, independent hook-identity
space and an independent hot-reload unit — which is exactly what an
isolation boundary should be.

**Drift indicators:**
- A `Call`/import path that mounts one instance's runtime inside
  another's execution.
- Scope-chain logic added to the VM or builder rather than the view
  layer.
- A reload that rebuilds a child instance's tree because a *parent*
  reloaded.

### 3. A view is a leaf or a branch, never both.

A **leaf** renders exactly one `instance` and contributes one layer. A
**branch** composes child views and renders no instance of its own.
"Own content" is never a privileged field beside children; it is simply
a leaf, which a branch may order like any other child.

**Why:** it makes z-ordering total and exception-free (every layer is a
child with an explicit order; there is no implicit "the instance is
underneath the children" rule), it makes `application` just the root
branch instead of a permanent special case, and it makes illegal states
unrepresentable (no "instance present but conflicting with slotted
children," no "neither set"). It matches the embedder's real
decomposition: spatial layout within a screen is done *inside one
instance* via imports (see §7); cross-view relationships are *layered*
(HUD ← overlay ← modal). Lifecycle follows from the variant — a leaf
exits with its instance; a branch exits when its children drain.

**Drift indicators:**
- A view type that holds both an `instance` and a child collection.
- A "frame" branch that grows its own layout description instead of
  being a leaf whose `.ogh` does the layout.
- Spatial, non-overlapping placement of one view *inside* another's tree
  (that is the deferred slot feature — see Deferred, below; it must
  arrive as a new explicit variant, not by re-merging leaf and branch).

### 4. Host state is scoped by lifetime and authority, not by domain.

State is organized into nested **lifecycle scopes** that mirror the view
tree: the always-live application/shell scope, then `session`, `map`,
combat, etc. — each owned and written by the host, read-only to `.ogh`
(§2 preserved). A value belongs to a scope by *when it exists and who
writes it*, never by what feature it concerns. The authority half of
that test also draws the boundary of host state itself: state the UI
*itself* authors — open/closed, active tab, scroll offset, text or a
rebind in progress — is in-language `state`, never host state at any
scope. Host state is what the *host* writes; presentation-local state is
not host state at all.

**Why:** the domain axis ("shell vs. game data") does not carve the
joint — wind is feature-specific *and* cross-cutting. The lifetime axis
does: wind is `session`-scoped world state; any view alive in a session
may read it (including to style every element), while the main menu
cannot, because the session scope is not live at menu time. Availability
becomes structural ("you cannot read a scope that is not an ancestor and
live") instead of a convention.

**Drift indicators:**
- A "shell vs. game" or "ambient vs. specific" partition of state by
  domain.
- A view reading a key from a scope that is neither its own nor an
  ancestor.
- State whose value depends on which *sibling* view is focused living in
  a shared ancestor scope.
- Presentation-local state the UI authors (visibility, active tab,
  scroll, in-progress text or rebind) pushed in as host state instead of
  held as in-language `state`. (In Untold Lore this is ~30% of the values
  threaded into a UI's per-frame `update` — `show_inventory`,
  `chat_visible`, `chat_input_active`, the `triggers_editor_active_tab`
  field — all of which belong in-language.)

### 5. Reads pull up the scope chain; a view pays only for what it reads.

A host-state read resolves leaf→root: the view's own scope, then each
ancestor, then the application/shell scope. Resolution is
**pull-by-reference** — a view's dependency set is exactly the keys it
references, so scope *size* is irrelevant to cost and invalidation tracks
read-sets. The chain is *borrowed and assembled during the synchronous
tick traversal*, never stored on views.

**Why:** pull-by-reference dissolves the "main menu must declare all of
the world's state" problem — the menu references nothing world-shaped
and pays nothing. It also dissolves prop-drilling of *lifetime-stable*
values: a registry, ruleset, or config is provided **once** at the scope
that outlives every reader and simply pulled, never re-threaded
per-frame. (Untold Lore today passes `ruleset`, `item_registry`,
`ability_registry`, and `cfg` into nearly every UI's `update` every
frame purely for lack of a scope to hang them on.) Assembling the chain
during traversal (rather than storing parent pointers) keeps the view
tree a clean ownership tree with no cycles, no `Rc`/`RefCell`, and no
aliasing; the borrow lives exactly as long as one render pass. This
generalizes INTENT §2's `GetState → GetHostState` fall-through up the
tree — but it lands entirely in the **view layer, with zero engine
change** (confirmed in implementation): a leaf resolves its declared
requirements against the borrowed chain and `inject_host_state_if_changed`
them into its runtime; the VM's `GetHostState` opcode still reads the
runtime's own host-state map, untouched. Pull-by-reference falls out for
free — only the keys a leaf's schema declares are ever resolved — and
diff-aware injection means an unchanged value costs nothing downstream.
The engine stays a single-instance black box (Tenet 2 holds *literally*).

**Drift indicators:**
- Scope-chain resolution pushed *into* the VM (a new opcode, a chain
  threaded through `GetHostState`) instead of resolved in the view layer
  and injected — the engine must stay untouched (Tenet 2).
- Parent back-pointers or `Rc<RefCell<Scope>>` to make ancestor reads
  work.
- Re-rendering every view when any scope changes (instead of only views
  whose read-set intersects the change).
- A scope chain cached across frames rather than rebuilt during tick.
- A value that outlives its readers (registry, ruleset, config)
  re-pushed every frame instead of provided once at the scope that
  outlives them and pulled by reference.

### 6. `host_state` is a typed *requirement on the scope chain*.

A view's `host_state { … }` block declares the typed shape it *requires
to be reachable in scope* — not a struct pushed into it by its immediate
owner. The framework resolves each requirement against the nearest
ancestor scope that provides a matching, type-checked value;
nearest-ancestor shadows farther ones. Providers are the host populating
a scope with a derived (`OghamState`/`OghamRecord`) struct; the
requirement↔provision match is verified when the view mounts.

**Why:** this is the unification that makes typed bindings
([`TYPED_BINDINGS.md`](TYPED_BINDINGS.md)) and the scope model *one*
mechanism instead of two. "This UI requires this struct as state"
becomes literal: declare the need by record type, and the chain provides
it — the same way ambient theme/wind resolve. It eliminates
prop-drilling of session-scoped data (player inventory provided once at
`session`, required by any descendant). The cost, paid deliberately:
the compile-time "every field populated" guarantee weakens to a
**mount-time** check, since the provider is found at runtime.

A requirement is **required by default**: an absent provider is a caught
failure (Tenet 10), never a silent default. A requirement that is
*legitimately* optional is declared optional (`T?`) and resolves to
`none`, which the author must handle in-language — there is no third,
silent path between "caught error" and "explicit `none`."

"Verified at mount" is literal and load-bearing: resolution happens
**before the instance's first execution**, not during it. A leaf that
requires host_state is constructed *deferred* — its requirements are read
off the schema (no execution needed), resolved against the chain, and
seeded into the runtime config; only then does the module run. So a
module that *reads* a required field can never execute against an
unprovided scope: a missing required provider fails the leaf at mount
*before any code runs* (it cannot crash on an unset field), and an
optional resolves to `none` first. A leaf the host has *already* built
(no required host_state, or values it supplied itself) mounts eagerly;
the two paths differ only in *when* construction happens, not in the
resolution rule. Because lifetime-nesting (Tenet 4) keeps an ancestor
provider live for the descendant's whole life, a requirement that
resolved at mount stays resolved — the check is needed once.

**Drift indicators:**
- A required-host_state leaf executed *before* its requirements resolve
  (so a missing provider surfaces as a runtime crash, not a mount-time
  caught failure).
- `host_state` treated as "what my constructor handed me" rather than
  "what must be in scope."
- Two parallel state-delivery systems (ambient pulled from chain; typed
  pushed per-instance).
- A missing provider surfacing as a render-time crash rather than a
  loud mount-time error caught at a boundary (Tenet 10).
- A silent default value standing in for an absent *required* provider
  (absence must be a caught failure or an explicit `T?`, never a default).
- Shadowing resolved farthest-wins, or by type alone when names collide.

### 7. Singular comes from scope; multiplicity comes from props.

Scope/`host_state` answers "**the** X in my context" — a singular,
ambient value resolved by type/name up the chain. **Props** (function
arguments to imported components) answer "**which** of several X" — and
are the only way to address N peers of a type, because a chain lookup
returns one value per name. Reusable components therefore take their
domain data as parameters and declare *no* scope requirement for it;
only context views declare `host_state`.

**Why:** it is forced by resolution semantics, not taste — two sibling
`inventory_grid`s in one scope would both resolve the same `inventory`.
A trade screen with two inventories holds two *named* fields
(`my_inventory`, `their_inventory`, both type `Inventory`) and passes
each as an argument; §9's path-based hook identity gives each call site
independent component state (hover/drag) for free. The same rule places
theme in scope (one) and list rows in props (many).

**Drift indicators:**
- A reusable component declaring `host_state { the_thing }`, capping it
  at one instance per scope.
- Disambiguating peers by spinning up extra views to rebind a scoped
  name, when they share lifecycle and state (props are correct there).
- Threading a singular ambient value (theme) through props instead of
  scope.

### 8. Reuse is by import; isolation is by view; events flow out, view-local.

Sharing presentation is *source-level composition* — `import` pulls
widget functions/records from another `.ogh` into one instance, with no
runtime boundary (this already carries Untold Lore: `colors.ogh` is
imported by 40 of 41 files). A separate `view`/`instance` exists **only**
for isolation: an independent scope, lifecycle, or hot-reload unit.
Events leave a view's instance (§2) and are handled by that view's
owner; cross-view coordination goes through shared scope state the host
writes, not a global event bus.

Outbound is a **single typed event channel** — the exact mirror of
inbound `host_state` (§6), carrying the view's declared `events` /
`OghamMsg` shape. There is no second return path: a host-side "outcome"
*is* a typed event the owner matches, not a separate value channel. This
collapses what an embedder otherwise grows two of — in Untold Lore,
construction-time handler closures that capture senders *and* six
ad-hoc `*Outcome` enums (`MapEditorOutcome`, `RulesetEditorOutcome`, …)
returned from `update`. One typed queue, drained by the owner each tick,
replaces both; emission can then see per-frame context because nothing
is pre-bound at construction.

**Why:** imports compose; boundaries isolate — conflating them creates
instances where a function call would do, or denies isolation where it
is needed. Keeping events view-local preserves §2's asymmetry and avoids
a tangled cross-view event bus; coordination-via-scope keeps the data
flow legible and one-directional. (A shared *module cache* — compiling an
imported module once and sharing its immutable protos across instances —
is a legitimate, model-invisible optimization, not a semantic feature.)

**Drift indicators:**
- A new `view`/`instance` created to *reuse a component* rather than to
  *isolate* state/lifecycle.
- A global event bus, or child→parent event bubbling across view
  boundaries, instead of coordination through scope state.
- An imported component carrying mutable per-instance state (imports
  should be pure: functions and records).
- A separate `*Outcome` return-value path running alongside the event
  channel — an outcome is just a typed event; there is one outbound
  mechanism, not two.
- Event handlers baked at instance construction (capturing senders) such
  that emission cannot see per-frame context — outbound drains a typed
  queue, it does not fire pre-bound closures. (In Untold Lore this
  pattern has metastasized to ~469 `with_event_handler` call sites —
  handler closures wired at build time, the outbound subset of which
  capture a `tx` sender — *plus* the six `*Outcome` enums on the return
  side; the call-site magnitude is the tell that emission has no
  per-frame home.)

### 9. One mechanism at every scale.

Reconciliation-by-key is the same algorithm at three nested levels:
widgets in a tree (the `Presence` widget), child views within a branch,
and top-level views within the application. Arrive → mount, depart →
exit-then-drop, replace-at-slot → strict swap, reorder → preserved by
key, rapid change → latest-wins, revert-mid-exit → cancel and unwind.

Those last two cases — "depart → exit-then-drop" (coexisting siblings)
and "replace-at-slot → strict swap" (one occupant at a time) — are not a
contradiction but **two policies of the one reconciler**, selected per
stack: `StackPolicy::Layered` for coexisting z-ordered layers (a HUD with
a modal above it; both live, exits fade in place) and `StackPolicy::Strict`
for single-occupancy where the incoming view does not mount until the
outgoing one's exit settles (fullscreen screen swaps — the exact semantics
UL's hand-rolled `OghamPresence` defined). A strict stack is one logical
slot; the application's top stack is strict (screens), with layered
branches above for overlays. (Multiple *independent* strict slots within
one stack is the deferred "slot" refinement, not this; see Deferred.)

**Why:** the embedder's hand-rolled instance state machine is just the
widget reconciler one scale up; unifying them means one set of
transition semantics to learn, test, and trust, and it lets exit
animations and frosted-glass backdrops compose across layers (a lower
layer has painted before an upper layer's backdrop samples it). The two
policies keep that single transition machine while admitting the only two
shapes an embedder actually needs (exclusive screens, coexisting layers),
rather than forking the reconciler.

**Drift indicators:**
- A second reconciler (or a bespoke "active screen" path) for one policy
  instead of both being modes of the same `ChildStack`.
- A view-stack reconciler with different transition semantics than the
  widget reconciler.
- Child-view matching by position when keys are present.
- A bespoke "active screen" swap that bypasses exit sequencing.
- The set of "kinds of active UI" enumerated in more than one place —
  worse, in places that *disagree*. (Untold Lore encodes it four times,
  keyed by the same kinds: an `OghamKey` enum of 23 variants, a
  `SessionOverlays` struct of 23 `Option<…UI>` fields, two mirror match
  tables of 23 arms each routing key→field, and an `OverlayState` enum of
  *13* variants — the 23-vs-13 mismatch is itself a drift bug surface
  every map between the two must paper over. One `ViewKey` set
  reconciled by one `ChildStack` replaces all four; a UI becomes a leaf
  view *registered*, not a variant *enumerated* in four parallel tables.)

### 10. Failure is caught at a boundary; it is never silent.

An unresolved requirement (Tenet 6) and an instance that faults at
runtime are the *same* event: the view enters a `Failed` phase, and the
failure propagates to the nearest enclosing **error boundary** — an
ancestor that designates a fallback child — which swaps the failed
subtree for that fallback. `Application` is the default root boundary,
so an uncaught failure is always *defined* behavior (an app-level
fallback plus a diagnostic naming the unresolved requirement and the
scopes searched) — never a blank surface, never a host-side panic. There
is exactly **one** failure path, and exactly **one** way to say "absence
is acceptable": a missing *required* provider is a caught error; a
*legitimately* optional value is an optional-typed requirement (`T?`)
the author destructures. Nothing renders against a silent default.

**Why:** Ogham's contract is predictable failure, one way to do any
task. A silent default — a label quietly stuck on a fallback value — is
the opposite: the author debugging "why is this wrong" gets no signal. A
boundary makes the failure loud and *located*. The model adds almost
nothing new because it rides what already exists: the swap is
reconcile-by-key (Tenet 9), the fallback is an ordinary child (Tenet 3 —
not a privileged field), and a boundary is just a branch that owns a
fallback child — there is no new `ViewBody` variant. It is also more
deterministic than its peers: React boundaries catch render-time throws,
whereas Ogham's failures are structural and resolved at *mount*, and
lifetime-nesting (Tenet 4) guarantees a requirement that resolved once
stays resolved for the view's whole life — a caught failure cannot
reappear mid-session. Folding instance runtime faults into the same
phase keeps it one mechanism rather than two, at the cost of
per-instance fault isolation in the tick loop — paid deliberately so a
faulting instance fails *itself*, not its siblings.

**Drift indicators:**
- A silent default value standing in for an absent required provider.
- An error boundary built as a new `ViewBody` variant rather than a
  `Failed` phase plus a fallback-role child on the existing reconciler.
- Two failure paths: instance runtime faults handled differently from
  unresolved requirements.
- An uncaught failure reaching the host as a panic, or blanking the
  surface, instead of resolving at `Application`'s root boundary.
- A tick loop with no per-instance fault isolation, so one faulting
  instance corrupts or freezes its siblings.
- "Optional" modeled as a required field with a silent default rather
  than an explicit `T?` the author must handle.

---

## Contract surfaces (illustrative)

Rough shapes to anchor the model — not final signatures.

```rust
// ── application ──────────────────────────────────────────────────────
// Singleton root: always-live shell scope + the services no view owns.
pub struct Application {
    shell:   Scope,          // theme, locale, viewport, dpi, fonts, input bindings
    top:     ChildStack,     // top-level views, reconciled by key; holds the
                             //   root Fallback child (default boundary, Tenet 10)
    watcher: SourceWatcher,  // §7 hot-reload pool over every live instance
    // surface passed into render(); dt + input passed into tick().
}

impl Application {
    pub fn tick(&mut self, dt: f32, input: &Input, desired_top: &[ViewKey]) {
        let chain = ScopeChain::root(&self.shell);   // shell roots every read chain
        self.top.reconcile(desired_top);             // host decides which top views exist
        self.top.tick(dt, input, &chain);            // thread the chain down
    }
    pub fn render(&self, surface: &mut dyn Surface) { self.top.render(surface); }
}

// ── view ─────────────────────────────────────────────────────────────
// A recursive scope. Leaf XOR branch (Tenet 3).
pub struct View {
    key:   ViewKey,    // stable identity among siblings (reconcile-by-key, §5)
    scope: Scope,      // this view's typed state slice; read-only to .ogh (§2)
    body:  ViewBody,
}

enum ViewBody {
    Leaf(LeafState),      // renders one source = one layer
    Branch(ChildStack),   // orders child views; renders nothing itself
}

// A leaf is built eagerly (host-supplied) or deferred until mount so its
// host_state requirements resolve before first execution (Tenet 6).
enum LeafState {
    Pending(LeafSpec),    // source + config; resolved, seeded, executed at mount
    Mounted(Instance),
}

// ── the recursive child manager (reconcile-by-key, one scale up) ──────
// Policy selects the two transition shapes of the one reconciler (Tenet 9).
struct ChildStack { children: Vec<Child>, policy: StackPolicy }  // ordered bottom→top
enum StackPolicy { Strict, Layered }  // exclusive screens | coexisting layers

struct Child {
    view:      View,         // recursive
    phase:     Phase,        // Mounting | Live | Failed | Exiting (the transition state machine)
    placement: Placement,    // z-order + optional layout box of this layer
    gating:    Gating,       // Modal (block input + freeze below) | PassThrough
    role:      Role,         // Normal, or Fallback shown when a sibling subtree Fails (Tenet 10)
}

enum Phase     { Mounting, Live, Failed, Exiting } // Failed = unresolved requirement or instance fault (Tenet 10)
enum Placement { Overlay { z: u32 /*, box */ } }   // layered; spatial slots are Deferred
enum Gating    { Modal, PassThrough }
enum Role      { Normal, Fallback }                // a branch with a Fallback child IS an error boundary (Tenet 10)

// ── instance (the type named `Ogham` today) ──────────────────────────
// The live rendering of one .ogh source. All of INTENT §1–§9 live here.
pub struct Instance {
    runtime: Runtime,        // compiled module + VM + this source's LOCAL state slice
    ui:      UI,             // reconciled widget tree (anim/hover/scroll/focus, §3)
    source:  SourceId,       // for hot-reload routing from Application.watcher
    config:  RuntimeConfig,  // per-source config (event contract, schema)
}

// ── scope + the chain assembled during traversal (Tenet 5) ────────────
struct Scope { state: HostState }   // host-written (Rust); read-only to .ogh (§2)

struct ScopeChain<'a> { scope: &'a Scope, parent: Option<&'a ScopeChain<'a>> }

impl<'a> ScopeChain<'a> {
    fn root(shell: &'a Scope) -> Self { Self { scope: shell, parent: None } }
    fn push(&'a self, scope: &'a Scope) -> Self { Self { scope, parent: Some(self) } }
    fn get(&self, key: &str) -> Option<&Value> {        // leaf → root, nearest shadows
        self.scope.state.get(key).or_else(|| self.parent.and_then(|p| p.get(key)))
    }
}
```

The reconcile/tick recursion: `reconcile` (cheap, chain-free) just
records the host's desired set — it can run at any level, so a branch's
children are reconciled by the host through it. The chain is assembled in
`tick`: `Application::tick` roots a `ScopeChain` at the shell and ticks
`top`; each view pushes its own scope and ticks under the extended chain
— a `Branch` recurses; a `Leaf` resolves its `host_state` requirements
via `ScopeChain::get`, injects them diff-aware, and re-renders. A
`Pending` leaf is *constructed here, at its first tick* — requirements
resolved and seeded before the module's first execution (Tenet 6). A
frozen strict pending (and a dormant fallback) skip the tick until
promoted/activated. A `Leaf` whose required requirement fails to resolve,
or whose `Instance` panics or errors (isolated per-instance), enters
`Failed`; the failure climbs to the nearest ancestor holding a
`Fallback`-role child, which the reconciler swaps in (Tenet 10).
`Application`'s root fallback bounds any failure that reaches the top.

(`View`, `ChildStack`, and `Application` are generic over the key type
`K` — the embedder supplies its own `ViewKey`; see Open Questions 5,
resolved.)

---

## Worked example: inventory, reused everywhere

Inventory is a **component**, not a view. The contexts (tab menu, shop,
DM tools, trade) are the views.

```ogh
// inventory.ogh — shared component; requires nothing from scope, takes data + behavior
record Item { name: string, count: int, icon: string? };
let inventory_grid = fn (inv, on_activate) {
  grid { children: inv.items.map(fn (it) {
    slot(it, background_color: colors.slot_bg,        // ambient theme ← scope (Tenet 7)
            mouse_down: fn () { on_activate(it.id); }) // behavior ← argument (Tenet 8)
  }) }
};
```

```ogh
// trade.ogh — one leaf view, TWO inventories (Tenet 7: multiplicity → props)
import "./inventory.ogh";
host_state { my_inventory: Inventory, their_inventory: Inventory };  // two named fields, same type

inventory_grid(my_inventory,    fn (id) { event("offer",   id); })
inventory_grid(their_inventory, fn (id) { event("request", id); })
```

```rust
#[derive(OghamRecord)] struct Item { name: String, count: i32, icon: Option<String> }
#[derive(OghamRecord)] struct Inventory { items: Vec<Item>, weight: f32, capacity: f32 }
#[derive(OghamState)]  struct TradeState { my_inventory: Inventory, their_inventory: Inventory }
```

- The Rust `Inventory` struct is the single source of truth; the `.ogh`
  `record` mirrors it and is **imported**, never re-declared per context
  (Tenet 8).
- The solo inventory menu instead reads the player's one inventory from
  the **session scope** as a requirement (`host_state { inventory:
  Inventory }`), resolved up the chain (Tenets 5–6) — singular, so it
  comes from scope, not props.
- The shop reads `player_inv` from `session` and `shop_stock` from its
  own scope: different providers, different depths, one rule.
- Each `inventory_grid(...)` call site gets independent hover/drag state
  via §9 — the two trade grids never share UI state.

---

## Deferred (architectural boundaries, not a roadmap)

These are *out of the end-state described here* and, if ever built, must
arrive as explicit additive features — never by relaxing a tenet.

- **Spatial cross-view embedding (slots).** Placing one view's tree
  *inside* another's layout (e.g. a federated/plugin panel docked in
  host chrome, not overlaid) requires a slot-hosting leaf and reopens
  the leaf/branch boundary (Tenet 3). Today all cross-view composition
  is layered; all spatial composition is intra-instance via imports
  (Tenet 8). This is the one capability the leaf/branch decision
  forecloses, accepted deliberately until a concrete federation need
  exists.
- **Function-signature typing.** Component parameters (the props in
  Tenet 7) are untyped today; only the `host_state` boundary and record
  field access are checked. Typing props is the natural extension that
  closes the last loose seam in the inventory example.
- The standing TYPED_BINDINGS non-goals (body type-checking, generics,
  sum types, removal of the loose path) carry over unchanged.

---

## Open questions

Genuinely unresolved; each needs a decision before implementation, none
changes the shape above.

1. **Failure diagnostic detail & debug/release parity.** Tenet 10
   settles the *shape*: a `Failed` view resolves at the nearest boundary,
   with `Application` as the default, so the "whole subtree vs. single
   view" question is answered ("the subtree under the nearest boundary").
   Residual: the exact diagnostic the root fallback renders — does it
   list every scope searched, the requirement's type, and the depths it
   expected a provider at? — and whether the fallback differs between
   debug and release (Flutter's red-screen vs. grey-box precedent).
2. **How a provider declares its typed shape.** A branch renders no
   `.ogh`, so its provided scope shape is declared Rust-side (the
   derived struct the host puts in the scope). Confirm that requirement↔
   provision matching reads the schema off those structs, and define
   what happens when a scope provides an *untyped* (loose) value a typed
   requirement asks for.
3. **`TypedOgham<S, M>` → typed view.** The existing typed handle maps
   onto a leaf view (`S` = its required scope shape, `M` = its emitted
   events). Settle whether the typed surface lives on `View`, on
   `Instance`, or on a `TypedView` wrapper, and confirm `M` is the
   *single* outbound channel (Tenet 8) that today's `*Outcome` enums fold
   into. *Drain mechanism decided:* **poll after `tick`** (not a handler
   invoked during it), so emission sees per-frame context; surface
   placement is the open part (lands with the UL outbound collapse).
4. **Drop vs. keep-warm policy** for a view leaving the desired set
   (per-view attribute, or branch-wide default). *(Current behavior: a
   departed view exits-then-drops; keep-warm is the additive variant.)*
5. ~~**Key type** for `ViewKey`.~~ **Resolved:** generic `K: Eq + Hash +
   Clone`, uniform across all scales (`View<K>`, `ChildStack<K>`,
   `Application<K>`); the embedder supplies its own key enum.
6. **Pure-scope branch state authorship.** *Confirmed first-class* (the
   implementation's proof has a branch providing a `session` value read
   by two leaf descendants); a branch and a leaf write their scope through
   the same host path (`scope.provide(...)`). Residual: the ergonomic API
   for the host to *update* a provider scope each frame (today a test-only
   accessor) — lands with the UL integration.

---

## Non-goals

- The view model does **not** own application state or the "which views
  exist" decision — only instances, their lifecycle, scope plumbing, and
  composition. The host declares the desired tree.
- It does **not** replace the in-language `Presence` widget; the two
  coexist and deliberately share one transition policy (Tenet 9).
- It does **not** modify any INTENT §1–§9 invariant; the engine
  (`instance`) is unchanged and the whole model is additive (Tenet 2).
- It does **not** bring the 3D world/scene render into the tree as a
  layer. The world is composited outside Ogham (GL framebuffer → Skia
  overlays → UI) and stays there. Admitting non-`.ogh` content as a leaf
  would reopen Tenet 3, so this is avoided until a concrete need — e.g.
  an upper layer sampling the world for a backdrop — genuinely cannot be
  met otherwise.
