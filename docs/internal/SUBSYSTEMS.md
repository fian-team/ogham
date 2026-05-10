# Ogham — Subsystem Map

> **Status: Live contract.**
>
> **Intent-level map. Pointers, not signatures.**
>
> Each section names a subsystem, the problem it solves, the
> invariants that should remain true as code changes (load-bearing
> ones get the full rule + *why* + drift-indicator treatment),
> where authority lives (the file/module that's the single source
> of truth), and a few file pointers to start reading from. Enum
> variants, struct fields, and method signatures are deliberately
> omitted — they rot. Read the source for current API.
>
> For a picture-shaped overview that locates these subsystems
> against the pipeline and the embedding seam, see
> [`ARCHITECTURE.md`](ARCHITECTURE.md).
>
> Cross-cutting tenets that span subsystems live in
> [`INTENT.md`](INTENT.md) and are referenced as `INTENT §N`.
> Subsystem-specific tenets are folded into the subsystem they
> govern, marked with the same **bold rule** + *Why* + *Drift
> indicators* structure.
>
> Subsystems whose section warrants the canonical-home treatment
> have their own dedicated file; the section here is a stub that
> names the load-bearing rules for grep-find and points at the
> dedicated file. Today, the live-contract subsystem docs are
> [`LANGUAGE.md`](LANGUAGE.md), [`VM.md`](VM.md),
> [`RUNTIME.md`](RUNTIME.md), [`WIDGET_TREE.md`](WIDGET_TREE.md),
> [`EVENTS.md`](EVENTS.md), [`FLEX.md`](FLEX.md),
> [`STYLE_AND_ANIMATION.md`](STYLE_AND_ANIMATION.md),
> [`ANIMATION_LIFECYCLE.md`](ANIMATION_LIFECYCLE.md), [`SURFACE.md`](SURFACE.md), and
> [`LSP.md`](LSP.md).

---

## Top-level facade

**Problem:** wrap the runtime, widget tree, file watcher, and font
collection as a single object the host application can hand to a
window/event loop without knowing the pipeline internals.

**Invariants:**
- The `Ogham` struct holds **one** runtime (`Arc<Mutex<Runtime>>`)
  and **one** UI; the two are kept in lockstep by `update()`.
- `tick(inject)` is the canonical per-frame seam: file-change
  check, host-state injection, conditional rerender + reconcile.
- `Runtime` is locked via `with_runtime_mut` so callers don't have
  to think about `Mutex` poisoning — `lock().unwrap_or_else(|e|
  e.into_inner())` is the recovery pattern.

**Authority:** `src/lib.rs`.

---

## Lexical analysis (scanner)

**Problem:** turn UTF-8 source into a flat token stream usable by
the parser and by the LSP.

This subsystem has its own document: [`LANGUAGE.md`](LANGUAGE.md).
The summary names load-bearing rules for grep-find purposes; read
`LANGUAGE.md` for full treatment.

**Invariants (one-liners; see `LANGUAGE.md` for full text and *why*):**
- The scanner is single-pass and never produces an error type
  outside `TokenType::Error(String)`. Errors are tokens, not panics.
- Comments (`//` and `/* */`) are consumed by the scanner and never
  reach the parser.
- Lines are 1-indexed, columns are 1-indexed, both for human
  diagnostics; byte offsets in `start`/`length` are 0-indexed.

**Authority:** `src/scanner/mod.rs` for the loop;
`src/scanner/token_type.rs` for the token enum.

---

## Parsing

**Problem:** consume tokens and produce a tree of `Statement` /
`Expression` nodes. Handle the widget literal / map literal /
match-arm-block ambiguities that come from reusing `{ ... }` for
all three.

This subsystem has its own document: [`LANGUAGE.md`](LANGUAGE.md).

**Invariants:**
- Recursive-descent precedence climb: `expression → logical_or →
  logical_and → equality → comparison → term → factor → exponent
  → unary → primary → parse_postfix`.
- `parsing_match_scrutinee` is the only piece of mode state; it
  exists to disambiguate `Identifier {` (widget) vs. `match
  Identifier {` (scrutinee + match block).
- Imports are top-level only; the parser refuses them in nested
  blocks (`parse_block(false)`).
- The "trailing expression without semicolon = implicit return"
  shape is folded into `expression_to_statement`, not into the
  expression grammar itself.

**Authority:** `src/parser/mod.rs` (`Parser`); `src/parser/*.rs`
for the AST node types.

---

## Bytecode compilation

**Problem:** lower the AST to a flat opcode stream that the VM can
execute, with cached compilation so rerenders skip the lowering
pass.

This subsystem has its own document: [`VM.md`](VM.md).

**Invariants:**
- `Compiler::compile_module` produces a `FunctionProto` whose
  trailing instructions look up `main` and call it; imports
  (`compile_import`) skip the `main` lookup and return a
  local-name → slot table for export extraction.
- Closures get an `UpvalueDescriptor` list at compile time so the
  VM knows whether to capture from the enclosing locals or
  upvalues.
- State variables are tagged at compile time (`is_state` on
  `Local`); assignments emit both `SetState` and `SetLocal`/
  `SetUpvalue`.
- `Compiler::stack_effect` is the bookkeeping for stack depth;
  every new opcode must contribute a correct effect or the
  compiler's depth tracking drifts and becomes useless for
  diagnostics.

**Authority:** `src/runtime/compiler.rs`; opcode shape in
`src/runtime/opcode.rs`.

---

## Bytecode VM

**Problem:** execute a `FunctionProto` against a `Runtime`,
producing the module's final value (a widget, typically).

This subsystem has its own document: [`VM.md`](VM.md).

**Invariants:**
- One `Vec<Value>` stack per VM, one `CallFrame` per active
  function call; both are bounded (`MAX_STACK_SIZE = 10_000`,
  `MAX_CALL_DEPTH = 1_000`, `MAX_ITERATIONS = 1_000_000`).
- Closures are `Rc<VMClosure>` — single-threaded reference
  counting is fine inside the VM. The runtime as a whole is
  `Send + Sync` only because callers wrap it in `Arc<Mutex<...>>`
  externally.
- `OpCode::Return` pops the current frame *and* restores the
  saved call stack / `has_branched` flag; new callable types
  (today: bytecode closures and bound mutation triggers) must
  preserve this invariant.

**Authority:** `src/runtime/vm.rs`; arithmetic helpers in
`src/runtime/ops.rs`.

---

## Runtime and host integration

**Problem:** own the long-lived state (host state, registered
event handlers, component state, import cache, compiled module
cache, widget registry, prelude) and provide the public API the
embedding application uses to interact with all of it.

This subsystem has its own document: [`RUNTIME.md`](RUNTIME.md).

**Invariants:**
- **Host state flows in, events flow out.** (`INTENT §2`.) The
  VM has `GetState`/`GetHostState` opcodes but no
  `SetHostState`. The one internal write path is import resolution.
- `set_host_state` diffs against the stored value and only
  `request_rerender`s when something actually changed — frame-rate
  injection from a host that hasn't changed anything is free.
- `event_handlers` are `Arc<dyn Fn(...) -> Result<Value, String>>`;
  `emit_event` returns `Ok(Value::Void)` for unregistered names so
  fire-and-forget `event(...)` calls always succeed.
- `compiled_module` is invalidated whenever `set_module` is
  called (which only happens on construction or hot reload).
- The prelude (`rgb`, `rgba`) executes once at construction and
  its bindings are lifted into `host_state`.

**Authority:** `src/runtime/mod.rs` (`Runtime`, `StateManager`,
`ImportResolver`); `src/runtime/config.rs` (`RuntimeConfig`);
`src/runtime/host_state.rs` (the `IntoHostValue` /
`HostStateSink` traits used at binding sites).

---

## Typed host bindings

**Problem:** give Rust hosts a typed wrapper around `Ogham` that
moves host state and host events through `#[derive(...)]`-
generated bridges instead of `HashMap<String, Value>` /
`Vec<Value>` plumbing — and emit a compile-time schema
manifest that the `ogham check` CLI / LSP can validate `.ogh`
files against.

**Invariants:**
- `TypedOgham<S, M>` wraps an `Ogham` plus the previous frame's
  `S`. `set_state(new)` diffs `new` against the previous `S` and
  only injects the changed top-level fields via
  `Runtime::inject_host_state_if_changed`.
- Host events delivered to the registered `OghamMsg` decoder turn
  into typed `M` values pushed onto a `VecDeque`; consumers drain
  via `poll_msg` / `drain_msgs`.
- The `#[derive(OghamState)]` / `#[derive(OghamMsg)]` macros each
  emit a manifest fragment (`binding_module`, top-level fields /
  variants and their Ogham types). The `ogham check` CLI loads
  the manifest at `.ogh` analysis time so a typo in
  `event("setvolume", v)` flags as an error before run.

**Authority:** `src/typed.rs` (`TypedOgham`, `OghamState`,
`OghamMsg`); `crates/ogham-derive/` (proc-macros);
`src/diagnostics/manifest.rs` (manifest format consumed by
`ogham check`).

---

## State management (component state)

**Problem:** persist `state` declarations across rerenders, keyed
by which call site declared them so that two calls of the same
component get distinct state.

**Invariants:**

- **State keys are call-stack paths.** A `state x = 0;` declared
  inside `fn counter()` called from `main()` becomes the key
  `main@1/counter@1:x`. The `@N` suffix disambiguates multiple
  calls of the same function from the same parent.

  *Why:* the React rules-of-hooks problem in disguise. Without
  per-call-site keys, two `counter()` calls share state. With
  them, each call gets independent state. The compiler's `Call`
  opcode increments a per-(parent, function-name) counter every
  time, and the closure's `captured_path` is restored on
  `Return`.

  *Drift indicators:*
  - State key generation that uses something other than the call
    stack (e.g. lexical position, hashed source).
  - A new opcode that calls a closure without going through the
    `Call` path (which is the *only* place call-counter
    increments live).
  - State that survives a `cleanup_unmounted_state` despite its
    component no longer being mounted.

- **`active_state_paths` is rebuilt per render; unmounted state
  is cleaned up.** A render that doesn't visit a state path
  drops it on `cleanup_unmounted_state`. Top-level state (empty
  path) is never cleaned up — the module is always mounted.

- **State searches walk up the call stack.** `find_existing_key`
  pops one frame at a time looking for an existing key, so
  `state` declared by a parent component is visible to children
  that don't redeclare. (This is unusual; most React-shaped
  systems would scope state purely to the declaring component.
  Worth interrogating in the design-review phase.)

**Authority:** `Runtime::state` (`StateManager` in
`src/runtime/mod.rs`); the state-handling opcodes
(`DeclareState`, `SetState`, `GetState` fallback) in
`src/runtime/vm.rs`.

---

## Lifecycle hooks (mount / unmount / effect / cleanup)

**Problem:** let `.ogh` code attach side-effects to a widget's
mount, unmount, and dependency changes — without the React
rules-of-hooks problem ("must be called unconditionally in the
same order every render").

**Invariants:**

- **Hook identity is path-based.** Every `(call_stack, hook_id)`
  pair is a fresh slot on the `StateManager`'s lifecycle registry;
  hooks survive reorders and re-renders without re-firing unless
  their dependencies actually change.

  *Why:* the same call-stack-path mechanism that keys component
  state. Re-using it means a `state` cell and an `effect` declared
  next to each other share an identity discipline; re-rendering a
  function from a different parent gives both a fresh slot.

  *Drift indicators:*
  - A new hook variant whose registry key uses something other
    than the current call-stack path.
  - A re-render path that increments the per-(parent, function)
    counter twice for the same call site (would shift hook
    identities and re-fire everything).

- **Mount fires inside `rerender` (pre-layout); unmount fires
  on drain.** `RegisterMountHook` opcodes stage onto
  `StateManager::pending_mounts`; `Runtime::pre_layout_drain`
  (called from inside `Runtime::rerender` after module
  execution) flushes the queue *before* the host's layout call.
  Mount bodies therefore cannot read post-layout sizes from the
  just-rendered tree — this is the M1 implementation
  trade-off, with post-layout mount timing on the backlog.
  Unmount fires when the widget owning the path-prefix
  drains — for widgets with an `exit:` style, that's after
  the exit animation settles and the next render's
  `process_drain_queues` consumes
  `UI.pending_drained_prefixes`.

- **Conditional hook registration is legal but warned.** The LSP
  emits a warning when an `on_mount` / `on_unmount` / `effect`
  appears inside an `if`, because conditional registration shifts
  hook identity. Body-level conditionals are encouraged instead.

**Authority:** `StateManager` in `src/runtime/mod.rs` (registry +
rotate / flush / cancel helpers); lifecycle opcodes
(`RegisterMountHook`, `RegisterUnmountHook`, `RegisterEffect`,
`RegisterEffectCleanup`) in `src/runtime/vm.rs`; parser support
in `src/parser/mod.rs` (`parse_mount`, `parse_unmount`,
`parse_effect`, `parse_cleanup`).

---

## Imports and modules

**Problem:** allow `.ogh` files to import bindings from other
files, with cycle detection and caching.

**Invariants:**
- Two import shapes: `import "./path";` (import-all, exposes every
  top-level binding) and `import [a, b] from "./path";` (named).
- Path resolution: explicit prefixes from `RuntimeConfig::with_import_path`
  win first; otherwise the path is joined onto the project root.
  Missing extensions get `.ogh` appended.
- Cycle detection uses a `loading_stack` of canonical paths;
  re-entering a path mid-load returns `VMError::ImportCycle`.
- Already-loaded imports are served from `cache`; the cache is
  invalidated only by constructing a new `Runtime` (i.e. hot
  reload).
- Imports execute in a fresh `VM` and produce an `Environment`
  whose top-level locals become the export set.

**Authority:** `Runtime::execute_import` in `src/runtime/mod.rs`;
import-bytecode handling in `OpCode::Import` in
`src/runtime/vm.rs`.

---

## Widget descriptors and the builder

**Problem:** turn `Value::Widget` produced by the VM into a live
`WidgetRef` tree, recursively, using a registry of factories so
custom widget types can be plugged in by the host.

This subsystem has its own document:
[`WIDGET_TREE.md`](WIDGET_TREE.md).

**Invariants:**
- `WidgetRegistry` is keyed by lowercased type name; the built-in
  set is `flex`, `text`, `textinput`, `svg`, `image`, `grid`,
  `presence`, `portal`. Host-registered widgets override built-ins
  on name collision.
- The builder is the *only* place a `WidgetRef` is constructed
  from a descriptor. The VM never reaches into widget internals.
  (`INTENT §1`.)
- Event listeners are wired up at build time: a property like
  `mouse_down: fn () { ... }` becomes a `Box<dyn Fn(&Event)>`
  closure that re-locks the runtime to call the bytecode closure.
  Drag listeners (`drag_start` / `drag_move` / `drag_end`) take
  the same shape but are wired through `register_drag_event_listener`
  so they can carry an `Event.payload` from the originating widget
  through dispatch.
- `WidgetDescriptor.owned_path` is captured by the builder from
  the VM's call-stack path at the descriptor's construction site
  and copied onto the produced widget's `owned_path_prefix`. This
  is what makes drain-time unmount work: when a widget drains, its
  prefix is what gets pushed into the tick's `drained_path_prefixes`.

**Authority:** `src/widget/builder.rs`.

---

## Reconciliation

**Problem:** absorb a freshly-built widget tree into the live one
without dropping animation, hover, scroll, or focus state.

This subsystem has its own document:
[`WIDGET_TREE.md`](WIDGET_TREE.md).

**Invariants:**
- **Widgets reconcile; they don't rebuild.** (`INTENT §3`.)
- **Reconciliation matches by `key`, falls back to position.**
  (`INTENT §5`.)
- `UpdateResult::REPLACE` means the parent should swap the
  `WidgetRef` out; `absorbed: true` means the live widget
  accepted the new descriptor in place.
- Keyless siblings are matched by position, *skipping* any
  exiting ghost — ghosts are never matched against new content.

**Authority:** `Widget::update` per widget type;
`FlexWidget::reconcile_children` in
`src/widget/flex_widget.rs` is the canonical implementation; other
containers either delegate to it or wrap a `FlexWidget`.

---

## Layout (flexbox)

**Problem:** resolve every widget's `Rect` from the available
space and the styles declared on each widget.

This subsystem has its own document: [`FLEX.md`](FLEX.md).

**Invariants:**
- Layout runs only when `needs_layout` is set or the dimensions
  change; debug builds warn at >5 calls/sec to catch over-marking.
- `Size` has four variants — `Fixed`, `Shrink`, `Grow(basis)`,
  `Percent(_)` (currently unimplemented; resolves to 0 inside
  `get_dimensions`).
- Wrap is row-only; column wrap is unsupported.
- Absolute-positioned children are excluded from the flex flow
  but still laid out and rendered.

**Authority:** `src/widget/flex_widget.rs`; style types in
`src/widget/style.rs`.

---

## Style transitions and animation

**Problem:** smoothly animate style property changes (hover,
state updates, entry/exit) using springs.

This subsystem has its own document:
[`STYLE_AND_ANIMATION.md`](STYLE_AND_ANIMATION.md).

**Invariants:**
- Animatable properties are exactly: `background_color`,
  `text_color`, `border`, `corner_radius`, `padding`, `margin`,
  `gap`, `text_size`, `opacity`, `transform`. Anything else snaps.
- Springs use sub-stepped semi-implicit Euler with a 1/120 s cap;
  large `dt` (e.g. tab-out) is stable.
- `effective_style` is what layout/rendering observes;
  `declared_style` is what the author wrote. They diverge only
  during a transition.

**Authority:** `src/widget/animation.rs` for the spring math;
`src/widget/flex_widget.rs` for the integration into reconcile /
hover / exit.

---

## Widget lifecycle (initial / exit / Presence)

**Problem:** support entry animations, exit animations, and a way
to sequence transitions between content generations (so a "page"
fully animates out before the next animates in).

This subsystem has its own document: [`ANIMATION_LIFECYCLE.md`](ANIMATION_LIFECYCLE.md).

**Invariants:**
- `initial:` is a snapshot the widget is born at; springs
  immediately retarget toward `declared_style`.
- `exit:` is a snapshot the widget animates *toward* while
  leaving the tree. Without it, a widget is dropped immediately
  on unmount.
- A `key` is required for exit animations to fire — without one,
  removal is indistinguishable from reorder. (`INTENT §5`.)
- `Presence { key, children }` holds new children as `pending`
  until every existing child has finished exiting; rapid key
  changes replace `pending` latest-wins; reverting the key
  cancels in-flight exits.
- `begin_exit` cascades: a widget without its own exit_style
  becomes a passive ghost if any descendant has a real exit
  animation.

**Authority:** `src/widget/flex_widget.rs` (own-exit, cascade,
ghost retention); `src/widget/presence_widget.rs` (generation
sequencing).

---

## Portal widget and layers

**Problem:** lift a subtree's paint and hit-test out of its
parent's clip / paint order into a named per-frame *layer*, so
modals, popovers, tooltips, drag previews, and toasts can render
above unrelated UI without authors having to hoist their state to
the root.

**Invariants:**

- **Portal contributes nothing to layout.** A `Portal` node has
  zero size in the parent's flow; its children are collected
  during the base-tree walk (Pass A) and painted during a separate
  layer pass (Pass B). Hit-testing walks open layers
  high-priority-to-low *before* the base tree.

- **Layers are a fixed, ordered set with declared policies.**
  `main` (0) → `overlay-modal` (100) → `popover` (200) →
  `tooltip` (300) → `toast` (400) → `cursor-attached` (500).
  Each layer carries a `BackdropPolicy` (`Block` blocks
  click-through and paints a translucent backdrop; `None` does
  neither) and a cursor preference (`Free` / `Inherit`).

  *Why:* a fixed set keeps the priority math obvious and prevents
  authors from inventing conflicting orderings. Per-layer
  policies (especially `Block`) collapse the "modal blocks input"
  pattern into a render-time concern instead of forcing every
  call site to wire its own `block_interactions` chrome.

  *Drift indicators:*
  - A new layer added with priority that overlaps an existing
    one.
  - A layer with `Block` policy whose backdrop fall-through is
    bypassed by some new dispatch path.
  - A widget that paints "above" another by reaching into the
    surface directly instead of declaring a layer.

- **Children paint at viewport-absolute coordinates.** Pass B
  uses each `PortalEntry`'s viewport-absolute rect, so a Portal
  nested inside a transformed/clipped ancestor still lands in the
  right place. (This was the Phase 2.5 M0 fix to a Phase 2 known
  limitation.)

- **Drag preview is a synthetic Portal entry.** While a drag is
  in flight, the Skia backend synthesizes a `cursor-attached`
  `PortalEntry` from `UI::active_drag_preview` so the preview
  follows the cursor without touching the live tree.

**Authority:** `src/widget/portal_widget.rs` (the widget itself);
`src/widget/portal_layer.rs` (`PortalLayers`, `PortalEntry`,
`BackdropPolicy`); `src/widget/mod.rs` (UI registration of layer
policies, `active_drag_preview` accessor); `src/skia.rs` for the
two-pass rendering (Pass A / Pass B) and backdrop-blur capture.

---

## Event dispatch

**Problem:** route mouse/keyboard/scroll events from the host's
window event loop into the right widget(s), and let widgets
distinguish "a real listener fired" from "an opaque container
returned `true`".

This subsystem has its own document: [`EVENTS.md`](EVENTS.md).

**Invariants:**
- Pointer events hit-test from the root downward; the first
  ancestor whose `contains_point` returns `true` continues into
  its children.
- Children are walked in declaration order; the first child that
  reports `handle_event = true` consumes the click for sibling
  iteration. **Painter order, not reverse-painter** — see open
  questions in [EVENTS.md](EVENTS.md).
- `EventContext::listener_fired` is the gate that lets an
  ancestor skip its own listener if any descendant fired one,
  while still letting `block_interactions` opaque containers
  return `true`.
- Hit-testing ignores `transform` (paint-only); the documented
  contract.

**Authority:** `src/widget/event.rs` (event types);
`UI::call_event` and `FlexWidget::handle_event` for dispatch.

---

## Drag and contextmenu dispatch

**Problem:** support real drag-and-drop with a typed payload, a
dead-zone before drag begins, declarative drop targets, an
optional cursor-attached preview, and right-click context menus —
without forcing every author to hand-roll the state machine on
top of `mouse_down` / `mouse_move` / `mouse_up`.

**Invariants:**

- **The host owns the dead-zone state machine; Ogham owns the
  hit-test and dispatch.** Ogham exposes
  `dispatch_drag_start` / `dispatch_drag_move` / `dispatch_drag_end`
  and `hit_test_drag_target` / `hit_test_drop_target`; the host's
  input pump decides when the cursor has crossed the drag origin's
  per-widget dead-zone (`drag_dead_zone:`, default 4px) and feeds
  the dispatchers a `DragState`.

- **Drag end resolves a drop target by walking layers
  high-to-low, then the base tree.** The deepest widget whose
  `accepts_drop(payload)` returns `true` wins. If none accept,
  `drag_end` fires on the originator (cancel-style behaviour).

- **`Event.payload` carries the drag payload through every drag
  dispatch.** Listeners read it via `payload` in their argument
  list; the `accepts_drop:` predicate sees the same value.

- **`contextmenu` is a separate dispatch path, not a
  reinterpreted `mouse_down`.** `Ogham::dispatch_contextmenu`
  fires on the deepest widget at the cursor (no automatic
  bubble). This keeps left-click / right-click routes
  independent.

**Authority:** `src/widget/event.rs` (`Event::payload`,
`DragState`, `EventContext::drag_state`); `Ogham::dispatch_drag_*`
and `dispatch_contextmenu` in `src/lib.rs`;
`UI::hit_test_drag_target` / `hit_test_drop_target` in
`src/widget/mod.rs`; drag/drop fields and listener registration on
`FlexWidget` in `src/widget/flex_widget.rs`;
`register_drag_event_listener` in `src/widget/builder.rs`.

---

## Drain-time unmount

**Problem:** make `on_unmount` / `effect` cleanups fire at the
*right* moment for widgets that exit-animate — after the exit
springs settle, not when the descriptor first disappears from the
declarative tree.

**Invariants:**

- **Drain queues are per-tick, owned by `UI`, consumed by
  `Runtime`.** `Widget::tick_animations` pushes drained-widget
  path prefixes into `TickContext.drained_path_prefixes` (and
  cancelled exits into `cancelled_unmount_prefixes`).
  `UI::tick_animations` moves them onto `UI.pending_drained_prefixes`
  / `UI.pending_cancelled_prefixes`. `Ogham::process_drain_queues`
  forwards them to `Runtime::process_drain_queues`, which fires
  the registered `on_unmount` / `cleanup` hooks under each prefix.

- **Cancellation undoes a pending unmount.** If a widget's `key`
  reappears mid-exit (Presence revert, conditional re-mount), the
  `cancel_exit` path pushes its prefix into
  `cancelled_unmount_prefixes`; the runtime drops the pending
  unmount instead of firing it.

- **Direct-Runtime users who don't tick a UI get a fallback.**
  `Runtime::flush_remaining_unmount_candidates` fires every
  pending unmount synchronously. Used by tests and by hosts that
  drive the runtime without a widget tree.

**Authority:** `src/widget/event.rs` (`TickContext`);
`UI::tick_animations` and pending-prefix fields in
`src/widget/mod.rs`; `Runtime::process_drain_queues` /
`flush_remaining_unmount_candidates` /
`queue_disappeared_unmounts` in `src/runtime/mod.rs`;
`Ogham::process_drain_queues` in `src/lib.rs`.

---

## Rendering (Surface)

**Problem:** turn the laid-out widget tree into pixels via a
backend-agnostic trait.

This subsystem has its own document: [`SURFACE.md`](SURFACE.md).

**Invariants:**
- **`Surface` is the only rendering seam.** (`INTENT §6`.)
- Widgets render in *parent-relative* coordinates; the surface
  walker translates the canvas before recursing.
- `RenderEffects` (opacity + transform) push around the widget
  *and* its descendants; pivot is the widget's layout center.
- **Rendering is two-pass when portals are open.** Pass A walks
  the base tree as usual, collecting any `PortalEntry`s into
  `UI::portal_layers`. Pass B walks each open layer in priority
  order at the layer's viewport-absolute coordinates, applying
  the layer's `BackdropPolicy` before painting that layer's
  children.
- `RenderContext` exposes optional `push_backdrop_blur` /
  `pop_backdrop_blur` scopes for `backdrop_filter` panels;
  backends without backdrop sampling get the trait's no-op
  default and the panel renders without the frost.

**Authority:** `widget::Surface` and `widget::RenderContext`
traits in `src/widget/mod.rs`; `src/skia.rs` is the reference
implementation; portal-layer two-pass walk in
`SkiaEnv::draw`.

---

## LSP server

**Problem:** provide hover, goto-definition, document symbols,
semantic tokens, and basic diagnostics for `.ogh` files in editors
that speak LSP.

This subsystem has its own document: [`LSP.md`](LSP.md).

**Invariants:**
- The LSP depends on `scanner` and `parser` only. It never
  instantiates a `Runtime` or builds a widget tree. (`INTENT §1`.)
- LSP positions are 0-indexed; Ogham spans are 1-indexed.
  Conversion happens at the boundary in `server.rs`.
- Diagnostics come from three sources: scanner `Error` tokens,
  parser `SyntaxError`, and lifecycle-hook conditional-registration
  warnings (`collect_lifecycle_warnings`). Schema diagnostics from
  `src/diagnostics/` are not yet wired through the LSP. The LSP
  doesn't try to recover from parse errors; one error truncates
  analysis for the document.

**Authority:** `src/lsp/server.rs` (capabilities, dispatch);
`src/lsp/document.rs` (per-document scan + parse).

---

## Schema diagnostics

**Problem:** catch drift between a `.ogh` file and the host's
typed-bindings manifest (top-level identifiers that no longer
exist, `event(...)` calls whose name / arity doesn't match the
declared message variants, etc.) at analysis time rather than
at runtime.

**Invariants:**
- The schema-diagnostic engine takes a parsed `.ogh` AST plus a
  manifest and emits `Diagnostic`s with `Severity` (Error /
  Warning) and span info. It never loads the runtime.
- The `ogham check` CLI is the canonical consumer today.
- LSP integration (surfacing schema diagnostics alongside
  scanner / parser / lifecycle warnings) is on the roadmap; the
  engine exists in `src/diagnostics/` but the LSP doesn't call
  it yet (it currently emits parse, scanner, and conditional-hook
  diagnostics only).

**Authority:** `src/diagnostics/check.rs` (the analysis pass);
`src/diagnostics/diagnostic.rs` (the `Diagnostic` /
`Severity` types); `src/diagnostics/manifest.rs` (the format
emitted by the derive macros). See
[`SCHEMA_DIAGNOSTICS.md`](SCHEMA_DIAGNOSTICS.md) for the design
discussion.

---

## File watching and hot reload

**Problem:** rebuild the runtime when any watched `.ogh` file
changes on disk.

**Invariants:**
- The watcher watches *parent directories* (recursive=false), not
  individual files, because most editors save via rename and
  per-file watches miss the new inode.
- The set of watched paths includes the main file plus every
  imported module the runtime resolved on the previous load.
  Adding a new import requires a reload to start watching it.
- Reload swaps in a brand-new `Runtime` and calls
  `UI::clear_lifecycle_state` on the existing `UI`. Component
  state, the lifecycle-hook registry, and any pending drain queues
  are dropped; widget tree state (animation / hover / scroll /
  focus) survives because it lives on the `UI` and reconciles.
  (`INTENT §7`.)

**Authority:** `src/file_watcher.rs`; `Ogham::reload` /
`Ogham::reload_file` in `src/lib.rs`.

---

## Standalone client (browser binary)

**Problem:** open a `.ogh` file in a window without an embedding
host — useful for examples and local development.

**Invariants:**
- The client is one consumer of the same `Ogham` library; it
  brings its own window/event loop and uses Skia.
- Nothing in `client/` should leak back into the library — the
  library has to remain embeddable in any window-system context.

**Authority:** `src/client/`.
