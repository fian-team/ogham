# Ogham — Cross-Cutting Design Intent

> **Status: Live contract.**
>
> This document records the cross-cutting principles that span
> Ogham's subsystems and don't have a single natural home in any
> one of them. Subsystem-specific tenets live alongside their
> subsystem and follow the same rule + *Why* + *Drift indicators*
> shape.
>
> If the code disagrees with this document, treat it as a **drift
> signal** worth investigating — don't silently update the doc to
> match. The whole point of writing intent down is to make drift
> visible. The right move is usually one of: (a) fix the code to
> match the intent, (b) update the intent because the design has
> genuinely changed and the team agrees, or (c) flag it as an open
> question.
>
> Drift indicators below each tenet describe what code patterns
> *would suggest* the intent has been quietly abandoned. Use them
> as a checklist when reviewing changes in unfamiliar areas.
>
> See [`ARCHITECTURE.md`](ARCHITECTURE.md) for a one-page picture-
> shaped orientation. See [`SUBSYSTEMS.md`](SUBSYSTEMS.md) for the
> subsystem map.

---

## 1. The pipeline is four distinct passes, not three.

Source → **scanner** (tokens) → **parser** (AST) → **compiler**
(bytecode) → **VM** (Values, including `WidgetDescriptor`s) →
**builder** (widget tree). Each stage has exactly one direction
of dependency on the previous. The VM never builds tree nodes;
the builder never sees AST; the compiler never holds runtime
state.

**Why this matters:** the seam between the VM and the widget tree
is what lets reconciliation work. The VM produces a fresh value
tree on every rerender; the builder turns that into nodes that the
*existing* widget tree can absorb. If the VM started constructing
nodes directly, every rerender would build a fresh widget tree and
animation/hover/scroll/focus state would be wiped each frame.
Likewise, having the parser short-circuit into bytecode would
couple syntax and execution semantics, and the LSP — which uses
the AST without ever running the VM — would have to either
duplicate the work or run user code to give hovers.

**Drift indicators:**
- Code in `runtime/vm.rs` that constructs a `widget::WidgetRef` or
  any concrete widget type.
- Code in `widget/builder.rs` that imports from `parser::` or
  reaches into AST nodes.
- A "fast path" that emits bytecode directly from tokens without a
  full AST (e.g. for arithmetic).
- The LSP holding a `Runtime` to answer a hover query.
- A new opcode whose handler short-circuits into building tree
  nodes instead of producing a `Value::Widget`.

---

## 2. Host state flows in, events flow out.

`.ogh` code can *read* host state via plain identifiers (the VM's
`GetState` opcode falls through to `GetHostState`) but cannot
mutate it. Mutation is the host's job, triggered by `event(name,
...)` calls leaving the runtime. There is no `SetHostState`
opcode. The *only* internal exception is import resolution, which
lifts an imported module's exports into host state at load time
(see `vm.rs`, `OpCode::Import`); that path is not reachable from
user code.

**Why this matters:** the asymmetry is what keeps the runtime
re-entrant. The VM never has to reason about handlers writing
state mid-execution and triggering a rerender from inside a
rerender. Mutations (`mutation("name").trigger`) provide the
request/response shape *without* breaking the asymmetry: the
trigger fires synchronously and the result is staged on the
mutation handle, but the VM still observes it on the *next*
render, not via a return value.

**Drift indicators:**
- A new `OpCode::SetHostState` (or any opcode that writes
  `runtime.host_state` from user code).
- An event handler whose return value is consumed synchronously
  by the VM at the call site (today, the VM discards the
  `event()` result and only mutations carry it forward).
- A `widget::Widget` that calls back into the runtime mid-render
  to push host state.
- Adding a synchronous `request_host_value(name)` primitive in
  `.ogh` code rather than going through `state` + `mutation`.

---

## 3. Widgets reconcile; they don't rebuild.

A rerender produces a fresh `Value::Widget` descriptor tree. The
[builder](WIDGET_TREE.md) turns that into a fresh `WidgetRef`
tree, but the live UI does not adopt it wholesale. Instead,
`UI::reconcile` calls `Widget::update` on the existing root,
which recursively absorbs the new descriptors in place. Same-type
matches copy props onto the live widget; same-key children
preserve identity across reorders; a new root for a subtree
implies replacement. Spring state, hover state, scroll position,
focus, and the widget's `Arc` identity all survive a successful
reconcile.

**Why this matters:** every animation in Ogham — hover springs,
entry transitions, exit ghosts, smooth scroll — depends on a
widget instance outliving the rerender that produced it. The
moment a rerender drops a widget and recreates it, its springs
reset to the new target with zero velocity, and authors see a
"snap" instead of an animation.

**Drift indicators:**
- A `UI::reconcile` rewrite that swaps roots based on equality
  rather than calling `update`.
- Any widget whose `update` returns `UpdateResult::REPLACE`
  unconditionally (= a no-op container that never absorbs).
- A new widget type that stores no animation/hover state on its
  own struct and re-derives it from props every frame.
- A child-matching change that ignores `key()` and falls back to
  position even when keys are present.
- Reconciliation that doesn't preserve `Arc` identity (e.g.
  always allocates a new `Arc<Mutex<...>>` for the matched child).

---

## 4. `Flex` owns the heavy machinery; other containers wrap it.

Layout, style transitions, hover, exit lifecycle, scroll, and
hit-testing all live on `FlexWidget`. `PresenceWidget` wraps a
`FlexWidget` and adds generation-keyed sequencing. `GridWidget`
specializes the layout. `TextWidget`, `TextInputWidget`,
`SvgWidget`, and `ImageWidget` are leaves that re-implement
*their own* style and event handling, but they don't try to be
flex containers.

**Why this matters:** Flex is 2700+ lines for a reason —
flexbox-with-wrap is hard, exit ghosts interact with reorder
matching, smooth scrolling needs per-frame ticks, and
hit-testing has to walk the same tree as layout in mirrored
coordinate space. Re-implementing any of that in another widget
is how you get subtle desync (e.g. a sibling that animates fine
but doesn't ghost on unmount, or a container that lays out
correctly but mis-routes scroll events).

**Drift indicators:**
- A non-Flex widget that grows its own `AnimationState` /
  `TransitionSet` field.
- A new container that re-implements `reconcile_children`
  instead of delegating to an inner `FlexWidget`.
- Multiple widgets independently implementing `begin_exit` /
  `cancel_exit` / `is_exit_complete` — they should cascade
  through Flex.

---

## 5. Reconciliation matches by `key`, falls back to position.

`key:` on a widget is its stable identity. Reconciliation builds
a `key → old_index` map first; new children with a matching key
adopt the matched old child even if its position moved. New
children without a key consume the next *unkeyed, non-exiting*
old child by position. Exiting ghosts are never matched by
position — they keep their slot independently of the live
match.

**Why this matters:** keys are what let exit animations work at
all. Without a stable identity, "this child was removed" and
"this child moved" are indistinguishable from
position-matching's perspective, so the framework would either
animate every reorder out (ugly and wrong) or animate nothing
out (kills the feature). The current design says: opt in to exit
animations *by* opting in to keys. It also lets state, hover,
scroll, and animation springs survive list reorders, which is
the React lesson re-learned.

**Drift indicators:**
- `key` documentation that calls itself "optional" in lifecycle
  contexts. (It is optional in general; for exit animations and
  for state preservation across reorders it is mandatory.)
- A reconciliation path that auto-generates keys from content
  hashes — that defeats the purpose, since identity then changes
  with content.
- Keys being matched by `==` between *different* widget types
  (the current code groups keys per parent's children list, not
  globally — keep it that way; making keys global would force
  authors to reason about uniqueness across the whole tree).
- A new widget that stores its own `key` field but forgets to
  override `Widget::key()`.

---

## 6. `Surface` is the only rendering seam.

Layout, hit-testing, animation, and reconciliation are
backend-agnostic and live in `widget/`. Only paint goes through
the `Surface` + `RenderContext` traits. The Skia implementation
is one consumer; custom backends are free to ship their own.
Logical (pre-DPI) coordinates flow through the widget tree;
backends multiply by their own DPI scale.

**Why this matters:** "we tightly couple the rendered output to
Skia" is exactly the failure mode the README warns against.
Today's drift risks come from convenience: a widget that wants
`skia_safe::Color` directly instead of `widget::style::Color`,
or a layout pass that calls `skia_safe::textlayout` for
measurement bypassing `LayoutContext`. Each one of those locks
out non-Skia backends.

**Drift indicators:**
- `use skia_safe::` outside of `src/skia.rs`,
  `src/widget/text_widget.rs`, `src/widget/text_input_widget.rs`,
  and `src/widget/svg_widget.rs`. (Text and SVG widgets
  currently *do* depend on Skia for measurement / parsing —
  this is a known seam-leak; document it as drift if it spreads
  further.)
- A `Widget::layout` implementation whose return depends on a
  Skia-specific helper.
- A `Surface` impl that mutates the widget tree (Surface should
  be read-only over the UI; UI mutates itself in `tick`/`reconcile`/
  `tick_animations`/`call_event`).
- Backends that need a side channel beyond `Surface::draw` to
  function (e.g. registering callbacks on the UI).

---

## 7. Hot reload preserves what it can, drops what it can't.

When the file watcher fires, the runtime is rebuilt from disk and
a fresh widget tree is constructed by the builder. The live UI
then `reconcile`s against it. Same-key, same-shape widgets
preserve animation/hover/scroll state; structural changes drop
state silently. The runtime's component-state map and the
lifecycle-hook registry are **not** preserved across a reload —
`Ogham::reload` swaps in a brand-new `Runtime` and calls
`UI::clear_lifecycle_state` to scrub any pending drain queues on
the UI side. Host-injected state is re-applied from
`RuntimeConfig`.

**Why this matters:** hot reload exists to be cheap and
convenient, not to be a checkpoint system. Trying to preserve
component state across reloads sounds nice until you realize the
shape of the program may have changed in ways that make the old
state ill-typed (e.g. a `state count = 0` was renamed to `state
counter = 0`). The current behavior is honest about that: the
visible UI snaps where the *new* program disagrees with the old.

**Drift indicators:**
- A code path that copies the old runtime's `state.component_state`
  or the lifecycle-hook registry into a new runtime on reload.
- A reload that bypasses `UI::reconcile` and rebuilds the tree
  from scratch — that would also drop animation/hover state on
  every reload, defeating the cheapness.
- A reload that *does* preserve state but silently coerces values
  across type changes.
- A reload path that skips `UI::clear_lifecycle_state` and lets
  the previous module's pending drain queues fire against the
  new module's hook registry.

---

## 8. The compiler is bytecode, not tree-walking. There is no
   tree-walk fallback.

`Compiler::compile_module` lowers the AST to a `FunctionProto`
once; from then on rerenders run `execute_module_cached` against
the cached bytecode. There is no AST-walking interpreter to fall
back on. Opcode behavior is the source of truth for the language;
the parser's job ends at producing a structurally valid AST.

**Why this matters:** maintaining two execution backends
(tree-walker + VM) doubles the surface for behavioral drift —
state-key generation, closure capture, branching flags, and
context-stack interaction would each need to match across the
two. Earlier history of the codebase did have a tree-walker;
removing it was load-bearing for sanity. References to "the
AST interpreter" in comments are historical.

**Drift indicators:**
- A new `runtime/tree_walker.rs` or any module that walks
  `parser::Statement` and produces `Value`s.
- A "debug mode" execution path that bypasses the VM.
- Compiler comments that defer behavior to "the interpreter" —
  the compiler *is* responsible for choosing what each construct
  becomes.

---

## 9. Hook identity is path-based, not order-based.

`state` cells, `on_mount` / `on_unmount` blocks, and `effect` /
`cleanup` blocks all key onto the call-stack path of the
declaring function plus a per-block hook ID. Re-rendering a
function from the same call site finds its hooks at the same
slot; calling the function from a *different* parent gets a
fresh slot. There is **no** "must be called in the same order
every render" rule — a hook inside an `if` is legal (just
warned, since whether it registers shifts identity).

**Why this matters:** React's rules-of-hooks problem is what
breaks when hook identity is order-based — a developer who puts
a hook inside a conditional silently corrupts every later
hook's identity, and the framework has to ban conditional
registration to make the model tractable. Path-based identity
sidesteps this: the call-stack path encodes "where in the
program this hook lives" *structurally*, so reorder of sibling
hooks within a function is fine, and a hook gated by an `if`
is just "registered or not" — there's no later hook whose
identity depends on it. This is the same mechanism that gives
component state independence across multiple calls of the same
component, and reusing it for hooks means the two systems
share a single discipline.

**Drift indicators:**
- A new hook variant whose registry key uses something other
  than the current call-stack path (e.g. a monotonic counter
  per render).
- A re-render path that increments the per-(parent, function)
  call counter twice for the same call site (would shift
  identities and re-fire every hook).
- A "rules of hooks" linter that bans conditional hook
  registration outright instead of warning — that constraint
  belongs to a different model.
- A new `Call` opcode variant that calls a closure without
  pushing a path frame; would silently fold callee hooks into
  the caller's identity space.
