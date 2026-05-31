# Ogham — Stage (multi-instance orchestration)

> **Status: SUPERSEDED by [`VIEW_MODEL.md`](VIEW_MODEL.md).**
> The host-side `Stage` orchestrator described below dissolved into
> the `application` / `view` / `instance` model: `Stage` became the
> root branch (`application`), `Screen` became `view`, and
> reconcile-by-key now runs at every scale. Read `VIEW_MODEL.md` for
> the current design. The transition-primitive analysis below remains
> accurate background but the proposed API is obsolete.
>
> ---
>
> <details><summary>Original (obsolete) proposal</summary>
>
> **Status: Proposed.** Not yet implemented. This document is the
> design contract for `Stage` — a host-side type that owns a
> keyed, z-ordered set of `Ogham` instances and sequences the
> transitions between them. It supersedes none of the existing
> live contracts; it adds an opt-in type that composes with the
> per-instance transition primitives already shipped
> (`begin_exit_root` / `is_exit_complete_root` /
> `restart_entry_animations` / `process_drain_queues` /
> `cancel_active_drag`, see [`src/lib.rs`](../../src/lib.rs)).
>
> Motivated by the reference embedder, untold_lore (UL), which
> hand-rolls this orchestration today in ~700 lines
> (`src/client/ogham_presence.rs`) plus the per-instance fan-out
> glue in `src/client/ui.rs`. See
> [`UL_ADOPTION_READINESS.md`](UL_ADOPTION_READINESS.md) §5
> (OverlayStack) for the consumer-side plan this design serves.

---

## Authority

- New type + trait: `Stage`, `Screen` (proposed home:
  `src/host/stage.rs`, re-exported from the crate root).
- Per-instance primitives it drives (already live):
  [`src/lib.rs`](../../src/lib.rs) —
  `begin_exit_root` (`:452`), `cancel_exit_root` (`:463`),
  `is_exit_complete_root` (`:473`), `restart_entry_animations`
  (`:491`), `process_drain_queues` (`:431`),
  `cancel_active_drag` (`:507`), `has_input_blocking_portal`
  (`:239`), `wants_cursor_free` (`:248`).
- In-language analogue: the `Presence` widget — see
  [`AGENTS.md`](../../AGENTS.md) "Presence — sequencing
  transitions between generations" and
  [INTENT §5](INTENT.md#5-reconciliation-matches-by-key).

---

## Motivation

Ogham is built to be embedded one interpreter per screen: each
overlay, editor, and HUD is its own `Ogham`, with its own
host-state namespace, its own event handlers, and independent
hot-reload. That is a deliberate property, not an accident —
keeping screens as separate instances is what lets an embedder
lazily construct them, drop their resources on close, and reload
one `.ogh` file without disturbing the others. **A real embedder
therefore always has many instances to juggle**, and the juggling
is identical from app to app.

Today the framework supplies the *primitives* for sequencing a
transition between two instances and stops there. The doc comment
on `begin_exit_root` says as much:

> Used by host-side orchestrators that sequence transitions
> between multiple Ogham instances (route swaps, modal stacks).

— i.e. "go build the orchestrator." UL did, in
`src/client/ogham_presence.rs`, whose own header describes it as
"a strict exit-then-mount state machine, mirroring the per-widget
`PresenceWidget` semantics at the Ogham-instance level." The cost
of leaving this to the embedder, measured on UL:

- A 23-variant key enum plus **two ~50-line mirror match tables**
  (`ui.rs:109-203`) that must stay byte-for-byte in sync — one
  for `&Ogham`, one for `&mut Ogham`.
- A **711-line transition state machine** (`ogham_presence.rs`)
  re-deriving latest-wins, revert-mid-flight, and
  drain-flush-before-demotion (`:246`) — all of which the
  in-language `Presence` widget already implements.
- A `mem::take` **borrow dance** (`ui.rs:271`) that exists only
  because the embedder is simultaneously the orchestrator and the
  instance registry, so the two `&mut self` borrows alias.
- An **80-line desired-key precedence cascade** (`ui.rs:20-80`)
  entangled with the borrow split and the fan-out.
- **21 ad-hoc `Option<…UI>` fields** (`substates.rs:165-204`)
  encoding "construct on open, drop on close" by hand.
- A `Loading → CampaignHub` **invisible-fade hack** (`ui.rs:64-72`)
  that lies about the desired key to drive an exit animation
  behind the loading screen — a workaround forced by the
  single-active model's inability to composite.

None of this is application logic. It is framework mechanism that
every embedder will re-derive. It belongs in Ogham.

---

## The tenet

> **Proposed INTENT tenet — Presence is one mechanism at two
> scales. The embedder writes policy and data, never lifecycle.**
>
> The same transition algorithm — match generations by `key`,
> exit the departed as ghosts, hold the arrived as pending, mount
> only when ghosts settle, latest-wins on rapid change, unwind on
> revert — governs both a widget subtree (`Presence`, in-language)
> and a stack of instances (`Stage`, host-side). An embedder names
> *which* instances are present and in what order, and supplies
> *what* data each one renders. It never drives `begin_exit` /
> `is_exit_complete` / drain ordering by hand.

**Why this matters:** an embedder that touches the lifecycle
primitives directly will, over time, re-derive a subtly different
transition policy than the in-language `Presence` — different
revert behavior, a missed drain flush, input routed to a
mid-exit ghost. UL's `ogham_presence.rs` is proof the divergence
is real work and a real correctness surface (it ships its own
regression tests for the drain-flush edge case). Collapsing the
two scales onto one implementation means the policy is specified,
tested, and evolved once.

**Drift indicators:**
- An embedder calling `begin_exit_root` / `is_exit_complete_root`
  / `restart_entry_animations` directly instead of through
  `Stage`.
- A host-side enum-keyed match table mapping keys to `&mut Ogham`.
- A `mem::take` / `mem::replace` on orchestration state to split a
  borrow against an instance registry.
- A "desired screen" computation that returns something other than
  a flat, recomputable description of the wanted stack.

---

## Conceptual overview

`Presence`, lifted one level. In-language:

```ogh
Presence { key: current_route_id, children: [ render_route(current_route_id) ] }
```

reconciles a *widget subtree* by `key`. `Stage` reconciles a
*z-ordered stack of `Ogham` instances* by key with the identical
algorithm — it is the widget reconciler ([INTENT §3](INTENT.md),
[§5](INTENT.md)) applied to instances instead of nodes.

The stack is z-ordered, not single-active. The bottom layer is
the persistent screen (a HUD, a world panel); higher layers are
overlays and modals composited on top. Single-active — UL's
current behavior — is the degenerate case of a height-1 stack.
This is the same shape as the planned OverlayStack in
[`UL_ADOPTION_READINESS.md`](UL_ADOPTION_READINESS.md) §5, and it
unlocks `backdrop_filter` frosted-glass overlays (see
[`SURFACE.md`](SURFACE.md)) because lower layers have already
painted by the time an upper layer's backdrop samples the canvas.

---

## Contract surfaces

### `Screen` — an instance plus how to feed it

A `Stage` owns its instances as `Box<dyn Screen>`. Each screen
owns one `Ogham` and knows how to inject its own per-frame data
from handles it captured at construction (snapshots, senders,
form state). Because the data dependency lives inside the screen,
the `Stage` can drive `tick` with no callback and no aliasing.

```rust
pub trait Screen {
    /// The instance this screen presents.
    fn ogham_mut(&mut self) -> &mut Ogham;

    /// Push this frame's application state into the instance.
    /// Called by the Stage for each *live* (non-frozen) layer
    /// before its update(). Reads from handles captured at
    /// construction; the default is a no-op for static screens.
    fn inject(&mut self) {}

    /// Does this layer block input and freeze the layers beneath
    /// it? `true` = modal (escape menu over a paused HUD);
    /// `false` = pass-through (a non-gating world panel). The
    /// Stage reconciles this with the instance's own
    /// `has_input_blocking_portal`.
    fn gating(&self) -> bool { true }
}
```

This re-homes, rather than invents, what UL already holds: a
per-UI struct with an `Ogham`, an `Arc<Mutex<FormState>>`, and a
per-frame `update(...)`. `update`'s body becomes `inject`; its
varying arguments become captured handles.

### `Stage` — the orchestrator

```rust
pub struct Stage<K: Eq + Hash + Clone> { /* … */ }

impl<K: Eq + Hash + Clone> Stage<K> {
    pub fn new() -> Self;

    /// Register a screen eagerly.
    pub fn register(&mut self, key: K, screen: Box<dyn Screen>);

    /// Register a factory; the screen is constructed on first
    /// appearance in the desired stack and (per policy) dropped
    /// when it leaves. Replaces ad-hoc `Option<…UI>` fields.
    pub fn register_lazy(&mut self, key: K, factory: impl FnMut() -> Box<dyn Screen> + 'static);

    /// Declare the wanted stack, bottom→top, recomputed each
    /// frame. The Stage diffs against the live stack by key.
    pub fn set_desired(&mut self, stack: &[K]);

    pub fn set_screen_size(&mut self, w: f32, h: f32);

    /// Per frame: reconcile the stack against the latest desired,
    /// advance transitions by `dt`, call `inject` + `update` on
    /// every live layer, run drain queues in the right order.
    pub fn tick(&mut self, dt: f32);

    /// Route an input event to the topmost gating layer (falling
    /// through pass-through layers). Gated against mid-transition
    /// ghosts.
    pub fn dispatch(&mut self, event: Event);

    /// Composite the stack bottom→top onto the surface.
    pub fn render(&mut self, surface: &mut dyn Surface);
}
```

---

## Stack reconcile semantics

`set_desired(&[K])` is declarative and idempotent, recomputed
every frame like UL's existing `desired_ogham_key()` — there are
no imperative `push` / `pop` calls to drift out of sync with
state. Each frame `tick` diffs the live stack against the desired
stack **by key identity** (the widget reconciler's rule,
[INTENT §5](INTENT.md), one level up):

- **Key arrived** (in desired, not live) → mount it *over* the
  current layers; its `initial:` entry animation plays. Lower
  layers are untouched. This is the additive case single-active
  cannot express.
- **Key departed** (in live, not desired) → `begin_exit_root`; the
  layer ghosts in place until `is_exit_complete_root`, then drops.
  Layers beneath become reachable again as it settles.
- **Key replaced at a depth** (e.g. `[A,B] → [A,C]`) → strict
  exit-then-mount *at that layer only*: `C` is held pending until
  `B`'s exit settles. This is UL's "strict everywhere" policy,
  now scoped to a slot rather than the whole screen.
- **Reorder** → preserved by key, exactly as keyed sibling
  reorders are in the widget tree.
- **Rapid change** → latest-wins: a new desired stack arriving
  while a layer is mid-exit replaces the pending content without
  it ever mounting. **Revert** → a departed key reappearing
  mid-exit calls `cancel_exit_root` and unwinds. Identical to
  `Presence`'s documented behavior.

This deliberately **supersedes the "strict everywhere" decision**
recorded in `ogham_presence.rs`: strict still governs same-slot
swaps, but pushes and pops composite. A direct payoff — it deletes
UL's `Loading → CampaignHub` invisible-fade hack (`ui.rs:64-72`):
the outgoing menu simply exits as a layer while the hub mounts
beneath it, no lie about the desired key required.

---

## Input routing and freeze

- **Input** dispatches to the topmost `gating()` layer; layers
  above a non-gating layer let unconsumed events fall through to
  the layer below, mirroring event bubbling. The Stage reconciles
  `Screen::gating()` with the instance's own
  `has_input_blocking_portal` (`lib.rs:239`) and surfaces
  `wants_cursor_free` (`:248`) from the top interactive layer.
- **Input is gated during a layer's transition** so clicks never
  reach a mid-exit ghost — the role UL's `input_target()` plays
  today.
- **Freeze**: layers beneath a gating layer skip `inject` and the
  animation tick (a paused HUD under the escape menu). Layers
  beneath a *non-gating* layer keep ticking (a live world panel).

---

## Rendering

`render` composites the live stack bottom→top in one pass; only
dirty layers re-layout. An upper layer's `backdrop_filter` samples
the already-painted lower layers (its documented contract,
[`SURFACE.md`](SURFACE.md)), which is the mechanism that makes
frosted-glass overlays work without the embedder freezing and
snapshotting the background by hand.

---

## Lifecycle and lazy registration

`register_lazy` formalizes UL's "construct on open, drop on close"
pattern (its 21 `Option<…UI>` fields, `substates.rs:165-204`) into
one policy: a screen is instantiated on first appearance in the
desired stack and, per a drop policy, torn down when it leaves the
stack. Drain queues and any in-flight drag are flushed
(`process_drain_queues`, `cancel_active_drag`) at teardown so a
re-mount starts clean — the drain-flush-before-demotion invariant
UL tests by hand (`ogham_presence.rs:246`) becomes a single
framework guarantee.

---

## The host loop, before and after

**Before** (UL, `ui.rs:262-284`):

```rust
let desired = match self.desired_ogham_key() { Some(k) => k, None => return };
let mut presence = std::mem::take(&mut self.ogham_presence); // borrow dance
let render_list = presence.step(desired, dt, self);          // self = registry
self.ogham_presence = presence;
for key in render_list {
    if let Some(o) = self.ogham_for_key_mut(key) {           // 50-line match
        o.set_screen_size(width, height);
        o.get_ui_mut().layout(width, height);
    }
}
// …plus a parallel input path, a parallel render path, manual drain plumbing
```

**After:**

```rust
self.stage.set_desired(&self.desired_stack());  // policy: which screens, ordered
self.stage.set_screen_size(width, height);
self.stage.tick(dt);                            // inject + update + transitions + drain
self.stage.dispatch(event);                     // routed to top gating layer
self.stage.render(surface);                     // composites the stack
```

---

## What stays in the embedder (the federation boundary)

`Stage` owns *mechanism*. The embedder keeps exactly two things,
both irreducibly application-specific:

1. **`desired_stack()` — which screens are present, ordered.**
   UL's `desired_ogham_key()` precedence (`ui.rs:20-80`) survives
   as a pure function returning `&[K]`, no longer entangled with
   borrows or fan-out.
2. **Each `Screen::inject` — what data the instance renders.**

This preserves the per-screen separation that motivated many
instances in the first place; `Stage` is the assembly step that
federates them, not a unification that collapses them into one
mega-instance with a shared namespace. It also pushes each screen
to declare the exact slice of application state it consumes (the
handles it captures), which is the god-state-slicing direction UL
wants independently (its refactor north star, INTENT §3 on the UL
side).

---

## Migration (UL)

- Delete `src/client/ogham_presence.rs` (the state machine becomes
  `Stage`).
- Delete the `OghamKey` mirror match tables in `ui.rs:109-203` and
  the `mem::take` in `ui.rs:271`.
- Convert each `…UI` struct to `impl Screen`: `new(...)` stays,
  `update(...)` becomes `inject(&mut self)` reading captured
  handles, `ogham_mut()` is the trait method, add `gating()`.
- Replace the 21 `Option<…UI>` fields with `stage.register_lazy`.
- `desired_ogham_key()` → `desired_stack() -> Vec<Key>` (now able
  to express HUD + overlay as a two-element stack, removing the
  loading-fade hack).

A good first migration to pressure-test the trait is
`EscapeMenu` over `DmHud`: it exercises a gating layer composited
over a frozen lower layer, input routed to the top, and the
lower layer resuming when the overlay pops.

---

## Open questions

- **Per-screen payload.** UL's `OverlayState::PlaceInspector(PlaceId)`
  carries data, but `OghamKey` deliberately drops it
  (`ogham_presence.rs`) and the payload lives in the embedder's
  overlay state. With `Stage` owning screens, the payload must
  reach the screen. Two candidate resolutions, **unresolved**:
  (a) the key carries it (`K` parameterized / non-`Copy`), which
  keeps payloads explicit at the `set_desired` site but complicates
  key identity for reconciliation; (b) the screen reads it from a
  captured handle (the same `inject` channel as all other data),
  which keeps keys clean but makes "which place is the inspector
  showing" implicit. Leaning toward (b) for identity simplicity,
  but it needs a concrete look at the inspector and DM-inventory
  cases before committing.
- **Drop policy granularity.** Is drop-on-leave a per-screen
  attribute (some screens keep-warm, e.g. the HUD; others drop,
  e.g. a rarely-used editor), or a single Stage-wide default?
- **Key type ergonomics.** Generic `K` vs. a fixed
  `enum`/`&'static str`. Generic preserves embedder type-safety;
  a string key trades safety for not threading `<K>` everywhere.

---

## Non-goals

- `Stage` does not own application state or the "which screen"
  decision — only the instances and their lifecycle.
- It does not replace the in-language `Presence` widget; the two
  coexist, deliberately sharing one transition policy.
- It does not introduce a new rendering seam — it composites
  through the existing `Surface` ([INTENT §6](INTENT.md)).

</details>
