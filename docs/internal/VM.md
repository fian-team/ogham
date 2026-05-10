# Ogham — Compiler and Bytecode VM

> **Status: Live contract.**
>
> The compile-to-bytecode and execute-bytecode pipeline. Front-end
> (scanner+parser) is in [`LANGUAGE.md`](LANGUAGE.md); the runtime
> object that owns the cached bytecode and supplies host state is
> in [`RUNTIME.md`](RUNTIME.md). The reconciliation that consumes
> the VM's output is in [`WIDGET_TREE.md`](WIDGET_TREE.md).

---

## At a glance

```
parser::Function (AST)
  → Compiler::compile_module     src/runtime/compiler.rs
      FunctionProto              (Chunk + constants + sub-protos + upvalue descs)
  → VM::run                       src/runtime/vm.rs
      Value                      (typically Value::Widget)
```

The compiler walks the AST once and emits a flat `Chunk` of
opcodes plus a constant pool. The VM walks the chunk with a
classic instruction-pointer + stack interpreter. Closures are
first-class; upvalues are explicit.

There is no AST-walking interpreter to fall back on — see
[INTENT §8](INTENT.md#8-the-compiler-is-bytecode-not-tree-walking-there-is-no-tree-walk-fallback).

**Authority:**
- Compiler: [`src/runtime/compiler.rs`](../../src/runtime/compiler.rs).
- VM: [`src/runtime/vm.rs`](../../src/runtime/vm.rs).
- Opcodes / function proto / upvalue / chunk:
  [`src/runtime/opcode.rs`](../../src/runtime/opcode.rs).
- Value enum: [`src/runtime/value.rs`](../../src/runtime/value.rs).
- Arithmetic ops: [`src/runtime/ops.rs`](../../src/runtime/ops.rs).

---

## Compiler

### What it produces

A `FunctionProto`:
- `name` (the literal `"<module>"` for top-level files,
  `"<import>"` for imported files, the source identifier for
  `let f = fn (...) { ... }` declarations).
- `arity` (parameter count).
- `chunk` (a `Vec<OpCode>`, a `Vec<Value>` constant pool, and a
  parallel `Vec<usize>` of source lines).
- `upvalue_count` and `upvalues` (an `UpvalueDescriptor` list
  describing how each upvalue is captured at runtime).
- `protos` (sub-function prototypes; `OpCode::Closure(idx)`
  refers into this).

### Module vs import compilation

Two entry points:

- `compile_module(module)` — compiles the body, then emits
  `GetLocal(slot_of_main); Call(0); Return`. Modules without
  `main` get `Void; Return`. The VM's top-level stack therefore
  ends with the result of `main()`.
- `compile_import(module)` — same body compilation, but no `main`
  lookup. Returns `(FunctionProto, Vec<(String, u8)>)` where the
  second element is `(top-level local name, stack slot)` so the
  caller can extract exports from the VM stack via
  `VM::read_stack_locals`.

### Tenets

- **`Compiler::stack_effect` is the bookkeeping for stack
  depth.** Every opcode declares its net stack effect (push -
  pop). The compiler tracks `stack_depth` so it can emit
  diagnostics about underflow at compile time.

  *Why:* compile-time stack tracking turns "I emitted
  `OpCode::Mod` but forgot to push its operands" from a runtime
  stack-underflow into a coding bug surfaced during compilation.
  Adding a new opcode without a correct entry quietly breaks the
  invariant.

  *Drift indicators:*
  - A new opcode added without an entry in the
    `stack_effect` match.
  - An opcode whose effect is "it depends" — split it into
    multiple opcodes with deterministic effects.

- **State variables are tagged at compile time.** `Local::is_state`
  records whether the corresponding `state` keyword was used.
  When an assignment resolves to a state local, the compiler
  emits `SetState(name_idx)` *and* `SetLocal(slot)` so both the
  persistence map and the stack slot stay in sync.

  *Why:* the VM doesn't know at runtime whether a slot is a
  state or a regular local. Splitting the decision into compile
  time keeps the `SetLocal` opcode simple. Upvalues are
  pessimistic — `compile_assign` emits `SetState` before
  `SetUpvalue` for *every* upvalue assign, because at compile
  time we don't have visibility into whether the parent slot
  was state. The VM's `SetState` is a no-op for non-state names
  (see "open questions" below).

  *Drift indicators:*
  - A runtime `is_state` lookup on every `SetLocal` (would
    defeat the point of the split).

- **Closures get an `UpvalueDescriptor` list at compile time.**
  Each entry is either `Local(slot)` (capture from immediately
  enclosing function's locals) or `Upvalue(idx)` (re-capture
  from the enclosing function's upvalues). The VM uses these at
  closure-creation time; no runtime introspection of the
  enclosing scope is needed.

- **Special-case opcodes for built-ins.** Some intrinsic calls
  are recognized syntactically by the compiler and lowered to
  dedicated opcodes:
  - `array.length()` → `ArrayLength` (peephole on `MemberAccess`
    callee).
  - `event(name, ...args)` → `EmitEvent(arg_count)`.
  - `mutation("name")` → `CreateMutation`.
  - `use_context("name")` → `GetContext(idx)` (the name is
    statically resolved to a constant-pool index — it must be a
    string literal).

  *Why:* keeping these as opcodes (rather than `Call`s through
  registered host functions) lets the VM run them without
  another stack frame, and lets the compiler enforce
  argument-shape invariants (e.g. `mutation()` takes exactly one
  string argument).

  *Drift indicators:*
  - Adding `event` / `mutation` / `use_context` as ordinary
    `Value::BytecodeClosure` bindings (they would lose their
    special compile-time validation).
  - A new built-in implemented as a registered closure that
    needed compile-time validation it can't express.

- **`Context { name, value, children }` is a compile-time
  scope.** When `Compiler::compile_expression` sees a widget
  literal whose identifier is `Context`, it lowers to:
  `eval(name); eval(value); PushContext; eval(children);
  PopContext; Widget(Flex, ...)`. The runtime widget produced is
  a transparent `Flex` containing the children.

  *Why:* contexts have to be visible during the *evaluation* of
  their `children` expression, not at render time. Lowering at
  compile time guarantees the push/pop bracket the entire
  children expression. Making `Context` an ordinary widget would
  push the scope only when the widget tree is traversed, which
  is too late — the children have already been evaluated by
  then.

  *Drift indicators:*
  - A new widget that needs compile-time scoping but isn't
    handled in `compile_expression`'s widget arm.
  - Pushing context at builder time instead of compile time.

### What the compiler doesn't do

- **No type checking.** Type annotations from the parser are
  ignored.
- **No constant folding.** `1 + 2` emits `Constant; Constant; Add`.
- **No dead-code elimination.** Unreachable branches still emit
  bytecode.
- **No peephole optimisation** beyond the built-in special cases.

These are deliberate omissions. Optimization passes are easier
to add when there's existing performance evidence.

---

## VM

### Execution model

Stack-based, single-threaded, single-frame-per-call. `VM::run`
sets up an initial frame for the top-level closure (no upvalues,
no captured path), then `execute(runtime)` loops:

1. `read_instruction()` advances the IP and returns the next opcode.
2. The opcode is matched and dispatched.
3. `OpCode::Return` either pops the frame and returns the result
   from the public `run()` (when frames is empty) or truncates the
   stack and pushes the return value into the parent frame.

`VM::call_closure(closure, args, runtime)` is the variant used
by event listeners — it pushes the closure + args, sets up the
frame manually, and runs `execute` on top.

### Resource limits

- `MAX_STACK_SIZE = 10_000` Values.
- `MAX_CALL_DEPTH = 1_000` frames.
- `MAX_ITERATIONS = 1_000_000` `Loop` opcodes per *frame* (the
  counter resets on each `Call`).

These are hard caps; exceeding any of them errors out cleanly
(`StackOverflow`, `CallStackOverflow`,
`ExecutionLimitExceeded`).

### Tenets

- **One `Value` stack, frame-relative slot indexing.** Each
  `CallFrame` carries `slot_offset`, the absolute index where its
  locals start. `OpCode::GetLocal(slot)` resolves to
  `stack[slot_offset + slot]`.

  *Why:* a single stack avoids per-frame allocation and lets
  upvalue capture refer to a stack index directly. A
  per-frame-stack design would force upvalues to either copy
  values or maintain frame-pointers — strictly more complex for
  no win.

- **Upvalues are open until closed.** Open upvalues hold a stack
  index and are observable through the live stack value;
  `CloseUpvalue` (emitted when a captured local goes out of
  scope) and `Return` (which closes everything at-or-above the
  frame's slot offset) move the value into a `Closed` variant.

  *Why:* this is the textbook closures-with-mutable-captures
  approach (Lox/Crafting Interpreters). It's load-bearing for
  `state` variables that get mutated through closures — see the
  state tests in `runtime/mod.rs`.

  *Drift indicators:*
  - A "fast path" that skips `close_upvalues` on return.
  - A new place that pops the stack without going through the
    helpers (`pop`, `Return`, `CloseUpvalue`, `Pop`,
    `truncate`) — closures referencing the popped slot would
    silently observe garbage.

- **`OpCode::Return` restores runtime call-stack state.**
  `Call` saves the current `runtime.state.call_stack` and
  `has_branched` flag onto the new frame. `Return` restores them
  before popping. New callable types have to follow this rule or
  state-key generation drifts.

  *Drift indicators:*
  - A new callable shape (a third arm besides
    `BytecodeClosure` and `BoundTrigger`) that doesn't save and
    restore the runtime state.
  - Bypassing `OpCode::Return` to exit a frame (e.g. by
    truncating frames directly).

- **Lifecycle opcodes key on the current call-stack path.**
  `RegisterMountHook`, `RegisterUnmountHook`, and
  `RegisterEffect` all use `runtime.state.current_path` plus
  the encoded `hook_id` as their registry key. The `hook_id` is
  assigned at compile time per source-position, so the same
  hook re-registers into the same slot on every render. This
  is what makes hook identity path-based, not order-based —
  see [INTENT §9](INTENT.md#9-hook-identity-is-path-based-not-order-based).

  *Drift indicators:*
  - A new lifecycle opcode that uses something other than
    `current_path` for its registry key.
  - A `Call` opcode variant that doesn't push a path frame
    (would silently fold callee hooks into the caller's
    identity space).

- **`Call` over a `BoundTrigger` is event dispatch, not
  function call.** When the callee is a `Value::BoundTrigger`
  (produced by `<mutation>.trigger`), `Call` doesn't push a new
  frame — it pops the args and the trigger, marks the mutation
  `Pending`, dispatches via `runtime.emit_event`, writes the
  result back into the mutation, and pushes `Void`. A rerender
  is requested.

  *Why:* mutations are the request/response shape that bridges
  the host-state-immutable rule. A `BoundTrigger` *looks* like a
  callable so authors can write `m.trigger(arg)` in `.ogh`, but
  it short-circuits the call into the host's event dispatch
  machinery.

  *Drift indicators:*
  - Asynchronous mutations (the current trigger is synchronous —
    by the time `Call` returns, the mutation status is already
    `Success` or `Error`).
  - A `BoundTrigger` returning a real value through the stack
    instead of via the mutation handle.

- **Recursive `WidgetDescriptor` construction is the hot path
  for rendering.** `OpCode::Widget` consumes `2 * property_count`
  stack values plus an identifier constant index, builds a
  `WidgetDescriptor`, and pushes a `Value::Widget`. Property
  keys must be `Value::String`; the compiler always emits them
  that way.

  *Drift indicators:*
  - A non-string property key (would produce a runtime error,
    but the compiler shouldn't emit one).
  - Building widget tree nodes (`WidgetRef`) inside
    `OpCode::Widget` (`INTENT §1`).

### Opcode catalog

Grouped by purpose. Operands shown in `()`; unannotated opcodes
take none.

**Constants and stack:** `Constant(u16)`, `True`, `False`, `Void`,
`Pop`, `CloseUpvalue`, `Dup`.

**Locals and upvalues:** `GetLocal(u8)`, `SetLocal(u8)`,
`GetUpvalue(u8)`, `SetUpvalue(u8)`.

**State and host state:** `GetState(u16)` (state → host state →
`screen_width` / `screen_height` → undefined), `DeclareState(u16)`
(uses persisted value if it exists, else initializer),
`SetState(u16)` (writes state and requests rerender),
`GetHostState(u16)`.

**Arithmetic:** `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Pow`,
`Negate`.

**Comparison and logical:** `Eq`, `Ne`, `Gt`, `Ge`, `Lt`, `Le`,
`Not`, `And`, `Or`. (And/Or do not short-circuit; both operands
evaluate. See "open questions".)

**Control flow:** `Jump(i16)`, `JumpIfFalse(i16)`, `Loop(u16)`
(unconditional backward jump with iteration counter increment),
`Return`.

**Functions and calls:** `Closure(u16)` (creates a `VMClosure`
from `protos[idx]`), `Call(u8)` (callable + N args on stack →
result on stack).

**Collections:** `Array(u16)`, `Map(u16)` (consumes 2N
key-value-pairs), `GetIndex`, `GetProperty(u16)`.

**Widgets:** `Widget { identifier_constant: u16,
property_count: u16 }`.

**Built-ins:** `ArrayLength`, `ArrayJoin`, `EmitEvent(u8)`, `Log`,
`CreateMutation`.

**Context:** `PushContext`, `PopContext`, `GetContext(u16)`.

**Imports:** `Import(u16)` (deserializes `ImportMeta` from a
string constant).

**For-expressions:** `BeginForExpr`, `AppendForExpr`,
`SpreadForExpr`. The compiler emits these around for-loops used
as expressions so the loop body's last value gets appended into
a result array.

**Branching flag:** `MarkBranched`. Emitted at the start of a
`match` expression. Used by `Runtime::state` to decide whether
state declarations after the branch are problematic. (See open
questions.)

**Lifecycle hooks (Phase 2):**
- `RegisterMountHook(hook_id: u16)` — pop a closure. If the
  current call-stack path is *new* this frame (not in
  `previous_active_paths`), enqueue `(path, hook_id, closure)`
  onto `pending_mounts`; otherwise drop the closure. Mount has
  no persistent registry — it queues inline and fires after
  reconcile + layout.
- `RegisterUnmountHook(hook_id: u16)` — pop a closure. Insert
  into `unmount_hooks` keyed by `(current_path, hook_id)`,
  overwriting any prior entry. Re-registered every render so
  the closure's upvalues reflect current scope. Fires when the
  path drains (see `Runtime::process_drain_queues`).
- `RegisterEffect { hook_id: u16, dep_count: u8 }` — pop
  `dep_count` values then a closure. Compare deps to
  `effects[(path, hook_id)].previous_deps`; if changed (or
  first run), schedule cleanup-then-fire. Always update the
  slot's closure so the body reflects current scope.
- `RegisterEffectCleanup` — pop a closure. Attach as the
  pending-cleanup of the currently-executing effect. Compile-
  time error if used outside an `effect` body (parser-enforced
  via `in_effect_body`).

### The Value enum as ABI

Values are the only thing that crosses between the VM, the
runtime, and the widget builder. The variants:

- `Integer(i32)`, `Float(f64)`, `Boolean(bool)`,
  `String(String)` — primitives.
- `Map(HashMap<String, Value>)`, `Array(Vec<Value>)` — composite.
- `BytecodeClosure(Rc<VMClosure>)` — single-threaded; safe
  because the VM owns the only `Rc` paths into them, and the
  runtime is wrapped in `Arc<Mutex<...>>` externally.
- `Widget(WidgetDescriptor)` — what the builder consumes.
- `Void` — unit.
- `Mutation(Rc<RefCell<MutationState>>)` — tracked event handle.
- `BoundTrigger(Rc<RefCell<MutationState>>)` — transient,
  produced by `m.trigger`, immediately consumed by `Call`.
  *Should never appear in stored state*; it's an
  "in-flight call" shape and storing it doesn't make sense.
- `WidgetRef(u64)` — Phase 2.5. An opaque widget identity
  allocated by `WidgetTree`, returned by the `focused_widget()`
  built-in and consumed by `focus(ref)`. Distinct from the
  `widget::WidgetRef` type alias (`Arc<Mutex<dyn Widget>>`)
  used inside the widget tree — this is a script-visible
  *identifier*, not a pointer.

`PartialEq` for `Value` is structural for primitives and
composites, identity (`Rc::ptr_eq`) for closures and mutations.

---

## Runtime ↔ VM contract

The VM borrows `&mut Runtime` for the duration of `run`. The
runtime fields the VM reaches into:

- `state` (`StateManager`) — read for `GetState`/`DeclareState`,
  written by `SetState` and `Call` (which pushes a new
  call-stack frame and increments the call-counter). Also owns
  the lifecycle-hook registry (`unmount_hooks`, `effects`,
  `pending_mounts`, plus drain-side `candidate_unmounts` /
  `cancelled_unmount_prefixes`) written by the four
  `Register*Hook` opcodes and consumed by
  `Runtime::process_drain_queues`.
- `host_state` — read by `GetHostState` and by `GetState`
  fall-through. Written internally by `OpCode::Import` to lift
  imported bindings.
- `event_handlers` — read by `EmitEvent` and by `Call` over a
  `BoundTrigger`.
- `screen_width` / `screen_height` — read by `GetState`
  fall-through.
- `context_stack` — pushed/popped by `PushContext`/`PopContext`,
  read by `GetContext`.
- `imports` — written by `OpCode::Import` via
  `runtime.execute_import`, which spins up a *fresh* `VM` to
  evaluate the imported module. Recursive entry into the runtime
  via a fresh VM is fine; what's not fine is two VMs sharing a
  runtime concurrently (single-threaded design).

---

## Open questions (for the design-review phase)

- **`And` / `Or` do not short-circuit.** Both operands evaluate
  before the result is produced. This is observable: a side
  effect on the right-hand side of `false && side_effect()`
  still runs. Probably wrong; cheap to fix with `JumpIfFalse`
  emission.
- **`SetState` for non-state variables silently no-ops.** The
  compiler pessimistically emits `SetState` before
  `SetUpvalue`; the VM's `SetState` writes to `component_state`
  unconditionally. If the name doesn't refer to a state
  declaration, the write goes through anyway, polluting the
  state map. Audit needed.
- **`Call` validates arity but not types.** Mismatched argument
  types fail somewhere downstream (usually a `TypeMismatch` from
  an arithmetic op). Could be an early check at `Call`.
- **No tail-call optimization.** Recursion bounded by
  `MAX_CALL_DEPTH = 1_000`.
- **`MarkBranched` exists but the constraint it once enforced
  was "no `state` declarations after a branch".** Verify the
  constraint is still enforced anywhere; if not, the opcode is
  dead weight.
- **`Range` expressions are not first-class values.** The
  compiler emits `Void` for any `Range` expression outside of a
  for-loop's range. Either lift them to `Value::Array(0..n)` or
  flag them as a parse error outside of for-loops.
- **`OpCode::Log` is a runtime-side `eprintln!`.** Useful for
  development, problematic for an embedded UI in a game
  (writing to stderr from a render-side closure). Should be a
  configurable sink.
- **Mutation triggers force a rerender unconditionally.** Even
  if the handler did nothing observable, a rerender is queued.
  Cheap to drop the `request_rerender` and let the handler
  decide via host_state writes.
- **`captured_path` on closures duplicates the runtime state's
  call stack.** The comment in `opcode.rs` explicitly says it
  mirrors the tree-walk interpreter's path. Now that the
  tree-walker is gone, audit whether the duplication is still
  needed or whether the runtime state is sufficient.
- **`OpCode::Import` lifts imports into `host_state` so that
  `GetState` finds them.** This is the one place the "host
  state is read-only from .ogh" rule has an internal exception.
  See `INTENT §2` and audit whether the cleaner design would be
  a separate `import_state` map (or just an `Environment`
  lookup before host state in `GetState`).
