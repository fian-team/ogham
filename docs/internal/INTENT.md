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
rerender. Edits never write host state from `.ogh`; they leave as
`event(name, ...)` calls carrying the change — for schema-described
state, a `(path, op)` the host applies via `editable::apply()` (see
§12) — and the VM observes the result on the *next* render, not via
a return value.

**Drift indicators:**
- A new `OpCode::SetHostState` (or any opcode that writes
  `runtime.host_state` from user code).
- An event handler whose return value is consumed synchronously
  by the VM at the call site (the VM discards the `event()`
  result).
- A `widget::Widget` that calls back into the runtime mid-render
  to push host state.
- Adding a synchronous `request_host_value(name)` primitive in
  `.ogh` code rather than reading host state ambiently and emitting
  an event.

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
specializes the layout. `TextWidget`, `TextInputWidget`, and
`ImageWidget` are leaves that re-implement *their own* style and
event handling, but they don't try to be flex containers.

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
the `Surface` + `RenderContext` traits, of which the Skia
implementation is the only one. Logical (pre-DPI) coordinates flow
through the widget tree; backends multiply by their own DPI scale.

**Why this matters:** the seam is **not** kept for backend
portability — Skia is the only impl and GPU/tile-baking lives
*outside* Ogham, so there is no second backend coming
(IDENTITY_AND_SCOPE §2). It survives as a **paint-isolation + test
seam**: it keeps `skia_safe` out of `widget/` so layout, hit-test,
and animation stay backend-pure and unit-testable, and it blocks
the convenience drift where a widget reaches for `skia_safe::Color`
instead of `widget::style::Color`, or a layout pass calls
`skia_safe::textlayout` for measurement bypassing `LayoutContext`.
The test seam is the live reason; portability is a lapsed one.

**Drift indicators:**
- `use skia_safe::` outside of `src/skia.rs`,
  `src/widget/text_widget.rs`, and
  `src/widget/text_input_widget.rs`. (Text widgets currently *do*
  depend on Skia for measurement — the one known seam-leak;
  document it as drift if it spreads further. `svg` is cut, so
  `svg_widget.rs` is gone.)
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
state silently. The runtime's component-state map is **not**
preserved across a reload — `Ogham::reload` swaps in a brand-new
`Runtime`. Host-injected state is re-applied from `RuntimeConfig`.

**Why this matters:** hot reload exists to be cheap and
convenient, not to be a checkpoint system. Trying to preserve
component state across reloads sounds nice until you realize the
shape of the program may have changed in ways that make the old
state ill-typed (e.g. a `state count = 0` was renamed to `state
counter = 0`). The current behavior is honest about that: the
visible UI snaps where the *new* program disagrees with the old.

**Drift indicators:**
- A code path that copies the old runtime's `state.component_state`
  into a new runtime on reload.
- A reload that bypasses `UI::reconcile` and rebuilds the tree
  from scratch — that would also drop animation/hover state on
  every reload, defeating the cheapness.
- A reload that *does* preserve state but silently coerces values
  across type changes.

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

`state` cells key onto the call-stack path of the declaring
function plus a per-cell ID. Re-rendering a function from the same
call site finds its cells at the same slot; calling the function
from a *different* parent gets a fresh slot. There is **no** "must
be called in the same order every render" rule — a `state` inside
an `if` is legal (just warned, since whether it registers shifts
identity).

> Lifecycle hooks (`on_mount` / `on_unmount` / `effect` / `cleanup`)
> are **cut** (IDENTITY_AND_SCOPE §2), so this identity now serves
> only `state`. The path-based *mechanism* stays — it is load-bearing
> for component-state independence and reorder survival; do not
> simplify it away on the grounds that only one consumer remains.

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

---

## 10. Ogham is a UI framework for Rust applications, not a standalone app platform.

The host (a Rust program) owns all state and logic; Ogham is a
projection surface over it — `.ogh` renders host state and emits
events. Ogham is *not* a JavaScript/React replacement or a
standalone application runtime. The four-pass pipeline (§1, §8) is
kept as real option value toward richer in-language logic later; the
standalone-app surface is not.

**Why this matters:** revealed preference across the real consumers
(small_mercies, untold_lore) is decisive — the render + language
core is heavily used while the standalone-app surface (typed
bindings, `mutation`, lifecycle, `client`-as-platform) was near-dead,
maintained, drifting code. Premature breadth is how challengers lose;
the way to displace an incumbent is to dominate one embedded niche
first. Conflating "UI for Rust apps" with "JS killer" made both worse
(IDENTITY_AND_SCOPE §1).

**Drift indicators:**
- New surface that presupposes "Ogham *is* the application" rather
  than a projection over a Rust host.
- A rival typed-host-state layer reappearing (the `OghamState` /
  `OghamMsg` shape) instead of the single `editable` vocabulary
  (§11, §12).
- `client` treated as a shipping platform rather than a dev
  previewer.
- Building toward the standalone-browser ambition on the game
  projects' budget instead of giving it its own roadmap.

---

## 11. Generically-edited host state is schema-described; everything else is plain; local state is ephemeral.

Three disciplines, divided by **who names the state into existence**:

- **Schema-described host state** — content that gets *generically*
  edited (e.g. small_mercies `Scene` / `Character`): described once
  via `#[derive(editable)]`, read and written through that one schema
  (§12).
- **Plain host state** — everything else the host wants on screen:
  injected as a `Value` directly (read in, events out, §2); no schema.
- **Local state** — view-invented, ephemeral (a mod's custom tabs,
  hover, an in-progress input buffer): a `state` cell (§9), *never*
  schema'd, because the host cannot name it.

Schema is **opt-in** for the generic-editing case, not a tax on every
screen.

**Why this matters:** the boundary keeps save/deep-link-relevant
state where it can be persisted (host-defined → host state) while
letting moddable UI invent its own structure (view-defined → local).
Universalizing the schema was tried and rejected — it turned a
bounded editor tool into a framework-wide tax (IDENTITY_AND_SCOPE §3).
One schema vocabulary (`editable`) avoids the rival-typed-state drift
that killed `OghamState`.

**Drift indicators:**
- Host-defined semantic state (current character, save-relevant
  selection) living in a local `state` cell — saves and deep-links
  silently lose it.
- A second schema / description system standing up beside `editable`.
- Schema-described state mutated in place from `.ogh` instead of via
  events the host applies (violates §2).
- A push to schema *all* host state "for uniformity."

---

## 12. Generic editing is one `editable` derive: read mirrors write, no serde in the UI path.

A `#[derive(editable)]` Rust struct round-trips through §2's
host-state-in / events-out with no per-type hand-written glue:

- **Read** projects `&T` into a *nested* `Value` node tree — true to
  the struct's structure, never flattened — that a recursive `.ogh`
  `field_node` renders; every container nests under a uniform
  `children`.
- **Write** leaves as a `(path, op)` event the host applies via
  `editable::apply()`.
- **Read and write come from the same derive** — read is the
  structural mirror of `apply`, so their paths agree by construction.
  Read is a structured `Reader` visitor with self-describing leaves
  (`scalar(&Kind, &str)`); **serde is never in the UI read path**. The
  union discriminant is the `$variant` sigil.

`editable` stays a pure leaf (serde only, no Ogham); Ogham stays
domain-agnostic; the **host composes** the two (the `Value`-building
visitor lives host-side). No second crate, no `OghamRead` /
`OghamWrite`.

**Why this matters:** the consumers proved the need by building
`editable` *outside* Ogham when the renderer-coupled typed layer
failed them (IDENTITY_AND_SCOPE §1). One derive driving both
directions is what prevents the read/write drift that recurs whenever
a second description of the same data exists (serde tagging vs. the
discriminant; `OghamState` vs. the renderer). A nested,
true-to-structure projection means new schemas just work without a
bespoke flattening.

**Drift indicators:**
- serde (`Serialize` / `serde_json::Value`) on the UI *read* path —
  a rival description that drifts from `apply` (the live example was
  the flat `schema_form.rs`).
- A second derive or schema system for the read (e.g. `OghamRead` /
  `OghamWrite` traits over `editable`).
- A flattened node projection (depth-tagged rows) instead of the
  nested tree.
- A `SetHostState`-style write path instead of events + host
  `apply()` (violates §2).
- `editable` gaining a dependency on the Ogham crate (the coupling
  that killed `OghamState`).
- Sentinel keys (`__add` / `__objdel`) reappearing instead of
  explicit container ops mapped 1:1 to `FieldOp`.
