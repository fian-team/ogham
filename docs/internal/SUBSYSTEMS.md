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
> [`LIFECYCLE.md`](LIFECYCLE.md), [`SURFACE.md`](SURFACE.md), and
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
  `presence`. Host-registered widgets override built-ins on name
  collision.
- The builder is the *only* place a `WidgetRef` is constructed
  from a descriptor. The VM never reaches into widget internals.
  (`INTENT §1`.)
- Event listeners are wired up at build time: a property like
  `mouse_down: fn () { ... }` becomes a `Box<dyn Fn(&Event)>`
  closure that re-locks the runtime to call the bytecode closure.

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

This subsystem has its own document: [`LIFECYCLE.md`](LIFECYCLE.md).

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

**Authority:** `widget::Surface` and `widget::RenderContext`
traits in `src/widget/mod.rs`; `src/skia.rs` is the reference
implementation.

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
- Diagnostics come from two sources: scanner `Error` tokens and
  parser `SyntaxError`. The LSP doesn't try to recover; one error
  truncates analysis for the document.

**Authority:** `src/lsp/server.rs` (capabilities, dispatch);
`src/lsp/document.rs` (per-document scan + parse).

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
- Reload swaps in a brand-new `Runtime`. Component state is
  dropped; widget tree state survives because it lives on the
  `UI` and reconciles. (`INTENT §7`.)

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
