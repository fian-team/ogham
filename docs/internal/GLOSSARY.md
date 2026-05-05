# Ogham — Glossary

> **Status: Live contract.**
>
> Vocabulary used across the contributor docs. When a word can mean
> two things in different parts of the codebase, this is where to
> disambiguate. Updates here ripple through the other internal docs.

---

## Pipeline stages

**Source** — A `.ogh` file's text. Always UTF-8.

**Token** — Output of the [scanner](LANGUAGE.md). One unit of
lexical syntax (e.g. `Identifier("counter")`, `Plus`, `Integer(5)`).
Carries `line`, `column`, `start`, `length` for diagnostics.

**AST** — Abstract syntax tree. Output of the [parser](LANGUAGE.md):
a tree of `Statement` and `Expression` nodes wrapping a top-level
`Function` (the *module*). Untyped; type identifiers are recorded
but not enforced.

**Module** — The `Function` produced by parsing a top-level `.ogh`
file. Always has arity 0; its body is the file's statement list.

**Bytecode** — Output of the [compiler](VM.md): a `FunctionProto`
holding a `Chunk` (a `Vec<OpCode>` plus a constant pool plus a
parallel `lines` vec for diagnostics). Sub-functions live in
`protos`. Cached on the runtime so rerenders skip recompilation.

**Closure** — A `VMClosure` = `FunctionProto` + captured upvalues +
the call-stack path captured at closure-creation time. Closures are
the only callable Value variant produced by user code.

**Upvalue** — A captured variable that has outlived its declaring
function. `Open` while the slot is still on the VM stack; `Closed`
once the enclosing scope returns and the value has been moved off
the stack.

**VM** — Stack-based bytecode interpreter (`runtime/vm.rs`). One
shared `Value` stack; one `CallFrame` per active call. Hard limits:
10 000 stack slots, 1 000 frames, 1 000 000 loop iterations per
function.

---

## Values and state

**Value** — The runtime's dynamic-type enum. Variants:
`Integer`, `Float`, `Boolean`, `String`, `Map`, `Array`,
`BytecodeClosure`, `Widget`, `Void`, `Mutation`, `BoundTrigger`.

**Map vs. struct** — There are no structs. Object-shaped data
(colors, transforms, transitions, widget styles) is `Value::Map`.

**Widget descriptor** — `Value::Widget(WidgetDescriptor)`. A
runtime representation of a widget literal: an identifier (the type
name) plus a `HashMap<String, Value>` of properties. Produced by
the `Widget` opcode; consumed by the [builder](WIDGET_TREE.md) to
construct widget-tree nodes.

**State** — Variables declared with `state name = expr;`. Persisted
on the runtime across rerenders, keyed by call-stack path.
*Component state*, in React terms.

**Host state** — Variables injected by the embedding Rust code via
`Runtime::set_host_state` / `inject_host_state_batch`. From `.ogh`
code these are observable (read via `GetState` falling through to
`GetHostState`) but immutable. See [INTENT §2](INTENT.md#2-host-state-flows-in-events-flow-out).

**Context** — A scoped value pushed onto a runtime-side stack by a
`Context { name, value, children }` widget literal and looked up
inside `children` via `use_context("name")`. Lexically scoped
to the children expression.

**Mutation** — A tracked event handle with status `idle | pending
| success | error`, returned by `mutation("event_name")`. Triggered
via `.trigger`; result data is read via `.data` / `.error` /
`.status` / `.pending`. The host-event request/response shape.

**Event** (in-language) — A fire-and-forget call: `event("name",
arg1, arg2)`. The handler's return value is discarded; tracked
flows go through mutations.

**Event** (in widget tree) — A `widget::event::Event` carrying a
name plus optional point/keyboard/scroll/value payload. Dispatched
to widget listeners during `UI::call_event`.

---

## Widget tree

**Widget** — A node in the live UI tree implementing the
`widget::Widget` trait. Long-lived: the same `Widget` instance
survives across rerenders when reconciliation finds a match.

**WidgetRef** — `Arc<Mutex<dyn Widget>>`. The shared, lockable
handle the tree is built out of.

**Builder** — `widget/builder.rs`. Converts `Value::Widget`
descriptors into `WidgetRef`s using a registry of factories
(`flex`, `text`, `textinput`, `svg`, `image`, `grid`, `presence`,
plus any custom widgets registered via `RuntimeConfig::with_widget`).

**Reconciliation** — The `Widget::update` walk. Compares the new
tree against the live tree, copies props in place where types
match, replaces subtrees where they don't. Preserves widget
identity (and therefore animation / hover / scroll state) by `key`,
falling back to position. See [WIDGET_TREE.md](WIDGET_TREE.md).

**Key** — Stable identity on a widget. Required for exit
animations and for preserving state across reorders. Keyless
siblings are matched by position and are intentionally
second-class.

**Ghost** — A widget that has been removed from the declarative
tree but is still in the live tree because it is playing an exit
animation. Drained on the next tick after its springs settle. See
[ANIMATION_LIFECYCLE.md](ANIMATION_LIFECYCLE.md).

**Pending** (Presence) — Children staged inside a `Presence`
container while the previous generation finishes exiting. Mounted
once no ghosts remain. Latest-wins on rapid key changes.

**Layout pass** — A full top-down `Widget::layout` walk that
resolves widget rects. Triggered by `needs_layout`; expensive
because text widgets call into Skia.

**Repaint pass** — Skia's `draw` pass. Runs every frame; the
`needs_repaint` flag is informational, not a gate.

**UpdateResult** — Returned by `Widget::update`. Carries
`absorbed` (did the existing widget accept the new descriptor in
place?) plus `needs_layout` / `needs_repaint` flags. Bubbles up
the tree so `Ogham::update` can decide whether to invalidate the
cached layout.

**TickResult** — Returned by `Widget::tick_animations`. Carries
`needs_repaint` / `needs_layout` / `still_animating`. Bubbles up
similarly.

---

## Style and animation

**Declared style** — The author-written target style on a Flex
widget. Held as `declared_style`; never overwritten by mid-flight
animations.

**Effective style** — `flex.style` — what layout and rendering
actually observe. Equals `declared_style` (or `hover_style`, or
`exit_style`) when settled, but holds interpolated spring values
during a transition.

**Target style** — Whichever of `declared_style`, `hover_style`,
or `exit_style` is currently being animated toward. Selected by
`FlexWidget::target_style()`.

**Initial style** — Optional snapshot the widget is born at on
first mount. Springs immediately retarget toward `declared_style`,
producing an entry animation.

**Exit style** — Optional snapshot a widget animates *toward*
while leaving the tree. Required for an exit animation; without
one, the widget is dropped immediately on unmount.

**Spring** — A unit-mass damped spring (`stiffness`, `damping`)
driving a single scalar. Sub-stepped semi-implicit Euler at up to
1/120 s per step. Settles when displacement < 0.01 *and* velocity
< 0.01.

**Transition** — An entry in the widget's `TransitionSet`
declaring that a specific style property should spring when its
target changes. Properties not listed snap.

---

## Surface and rendering

**Surface** — The trait an embedder implements to render the
widget tree. One method: `draw(&mut self, ui: &mut UI)`. Skia is
one implementation (`SkiaEnv`); custom backends are free to provide
their own.

**RenderContext** — The trait widgets call from their `render`
method (`fill_rect`, `draw_text`, `push_clip_rect`, …). The
backend implements this.

**RenderEffects** — Optional opacity + transform + pivot pushed by
a widget's `render_effects()` and applied for the widget *and its
descendants*. Paint-only — does not affect layout or hit-testing.

**Logical vs. physical pixels** — Widgets work entirely in logical
(pre-DPI) coordinates. The Skia backend multiplies by `dpi_scale`
inside `RenderContext` calls.

---

## LSP and tooling

**ogham-lsp** — The language server binary. Reuses the scanner
and parser; never instantiates the runtime or widget tree. See
[LSP.md](LSP.md).

**Document** — A scanned + parsed source buffer held by the LSP's
`DocumentStore`. Stores the latest `tokens` and (best-effort)
`ast`.

**Hot reload** — File-watcher-driven recompilation. Triggers a
fresh `Runtime::from_file` and a tree reconcile against the new
output; *some* state survives if keys and shapes match. See
[RUNTIME.md](RUNTIME.md).
