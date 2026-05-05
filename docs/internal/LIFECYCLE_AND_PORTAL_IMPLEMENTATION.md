# Lifecycle + Portal — Phase 2 Implementation Plan

> Companion to [`LIFECYCLE_AND_PORTAL.md`](LIFECYCLE_AND_PORTAL.md)
> (the design contract) and
> [`LIFECYCLE_AND_PORTAL_UL_AUDIT.md`](LIFECYCLE_AND_PORTAL_UL_AUDIT.md)
> (the consumer audit). This document specifies the per-merge
> work to land Phase 2: six merges (M0–M5), each independently
> shippable behind a validation gate, committed directly to
> `main` per the post-Phase-1 single-branch convention.
>
> Estimated total: ~2,500–3,500 LOC across implementation +
> tests, similar density to Phase 1's ~3,800 LOC across seven
> merges.
>
> The structure mirrors `TYPED_BINDINGS_IMPLEMENTATION.md` —
> per-merge goals + deliverables + steps + test matrix +
> dependencies + open-question resolution; then cross-cutting
> concerns, a consolidated risk register, a timeline, and the
> decision points required before M0 starts.

---

## At a glance

| Merge | Title | LOC | Tests | Risk | Hardest part |
|---|---|---|---|---|---|
| M0 | Lifecycle plumbing (foundation) | ~600 | ~12 | Medium | `owned_path_prefix` widget-tree migration |
| M1 | `on_mount` / `on_unmount` in `.ogh` | ~500 | ~15 | Medium | Closure scope capture for unmount |
| M2 | `effect` + `cleanup` | ~400 | ~12 | Low | Dep equality on `Value` |
| M3 | Portal: deferred-paint primitive | ~600 | ~10 | High | Two-pass paint without breaking existing renderer assumptions |
| M4 | Portal: `focus_trap` + `has_input_blocking_portal` | ~350 | ~8 | Medium | Focus stack semantics under modal-on-modal |
| M5 | UL validation pass + docs + worked examples | ~400 | ~5 (integration) | Medium | Hot-reload + lifecycle interaction (audit OQ#6) |

**Total: ~2,850 LOC implementation + ~62 tests.**

### Sequencing rationale

M0–M2 ship the **lifecycle subsystem** end-to-end. After M2,
authors can write `on_mount`, `on_unmount`, `effect`, and
`cleanup` in `.ogh`; the runtime fires them; the LSP
highlights and diagnoses them. This is the foundation —
Portal builds on it.

M3–M4 ship the **Portal subsystem**. M3 is the deferred-paint
primitive (the largest single mechanical change in the
phase); M4 adds `focus_trap` and the `has_input_blocking_portal`
runtime API.

M5 is **validation**: migrate three UL UIs (Settings save,
escape menu Portal, one tooltip), ship the
`examples/portals/components.ogh` reference library
(`Modal()`, `Tooltip()`, `Dropdown()`), graduate the design
doc from "Live design contract" to "Live contract."

### Validation gate at each merge boundary

Per the single-branch workflow, each merge commits directly
to `main` after passing its gate. The gate is **all of**:

- `cargo build --workspace` clean
- `cargo test --workspace` green
- All new tests added in this merge pass
- UL builds clean against the new `ogham` (`cd ../untold_lore && cargo build`)
- No regressions in existing tests

If a gate fails, the work-in-progress stays uncommitted until
the issue is resolved. No "ship and fix forward" — Phase 1's
clean-merge cadence held up; we keep that.

---

## Merge 0 — Lifecycle plumbing (foundation)

### Goal

Land all the runtime + widget-tree machinery for lifecycle
events **without exposing any author surface**. Nothing in
`.ogh` changes. The diff is observable via direct unit tests
on the new internal APIs.

### Deliverables

- `StateManager` field additions: `previous_active_paths`,
  `unmount_hooks`, `effects`, `pending_mounts`,
  `pending_unmounts`, `pending_effect_fires`,
  `pending_effect_cleanups`, `candidate_unmounts`,
  `lifecycle_active`.
- `EffectSlot` struct.
- Per-render diff machinery: rotate `active_state_paths` →
  `previous_active_paths` at frame start; compute mounted /
  unmounted sets on demand.
- `Widget::owned_path_prefix() -> &str` trait method (default
  implementation returns `""`).
- `Widget::flush_lifecycle_on_drain(&self, state: &mut StateManager)`
  trait method that walks the maps for matching prefixes and
  populates `pending_unmounts` / `pending_effect_cleanups`.
- Hook into `drain_exited_children` (`flex_widget.rs:431`)
  to call `flush_lifecycle_on_drain` on each drained widget.
- Hook into `cancel_exit` (`flex_widget.rs:1476`) to clear
  `candidate_unmounts` for the cancelled widget's prefix.
- The four new opcodes added to `OpCode` enum but
  **unimplemented in the VM** — the bytecode shape is
  reserved; M1/M2 fill in handlers.
- Module-level `lifecycle_active: bool` on `Runtime`,
  computed at compile time by walking the bytecode.
- `Call` opcode handler gate: `if runtime.lifecycle_active
  { active_state_paths.insert(path) }`.

### Implementation steps

1. **Add new `StateManager` fields** (`runtime/mod.rs:52`).
   Initialize all to empty in `StateManager::new()`. Run
   existing tests; nothing should break.
2. **Add `EffectSlot` struct** in `runtime/mod.rs`. No
   constructor variations needed; initialize in-place.
3. **Add the four opcodes** to `OpCode` enum in
   `runtime/opcode.rs`. Add unimplemented `_ => unreachable!()`
   arms in the VM dispatch — keeps the build green without
   exposing them yet.
4. **Add `Widget::owned_path_prefix`** with default `""`
   implementation in `widget/mod.rs`. Most widgets use the
   default; only `FlexWidget` needs to record its prefix
   (it's the function-call container in practice).
5. **Wire `FlexWidget` to record `owned_path_prefix`** at
   construction. The path is the call_stack at the moment
   the widget is built (mirror `VMClosure::captured_path`).
   Field: `owned_path_prefix: String` on `FlexWidget`.
6. **Add `flush_lifecycle_on_drain`** to `Widget` trait.
   Default: walks both `unmount_hooks` and `effects` maps
   for keys whose path starts with `self.owned_path_prefix()`,
   moves them into pending queues.
7. **Hook `drain_exited_children`** to call
   `flush_lifecycle_on_drain`. Order: deepest widgets
   drained first (existing recursion does this).
8. **Hook `cancel_exit`** to clear `candidate_unmounts` for
   the cancelled widget's prefix.
9. **Add `lifecycle_active` to `Runtime`** as a `bool`,
   default `false`. The compile pass sets it.
10. **Modify the compiler** (`runtime/compiler.rs`) to scan
    its emitted bytecode after compilation and set
    `lifecycle_active = true` if any of the four opcodes
    appears. Wire to `Runtime::set_module`.
11. **Modify `Call` opcode handler** (`runtime/vm.rs:539`)
    to conditionally insert the current path into
    `active_state_paths` based on `runtime.lifecycle_active`.
12. **Per-frame rotation** in `Runtime::rerender`
    (`runtime/mod.rs:369`): rotate `active_state_paths` →
    `previous_active_paths` at frame start (before clearing).
13. **Pre/post-layout drain hooks** added to the layout
    pipeline (caller of `UI::layout`). For M0, these are
    no-op stubs; M1 fills them in. Stub locations:
    - `Renderer::pre_layout_drain(&mut Runtime)` — drains
      `pending_unmounts`, `pending_effect_cleanups` (both
      empty in M0).
    - `Renderer::post_layout_drain(&mut Runtime)` — drains
      `pending_mounts`, `pending_effect_fires` (both empty
      in M0).

### Test matrix (~12 tests)

`tests/lifecycle_plumbing.rs` (new):

1. `state_manager_rotates_active_paths_per_frame`
2. `previous_active_paths_correctly_tracks_last_render`
3. `lifecycle_active_flag_false_for_module_without_hooks`
4. `lifecycle_active_flag_true_when_compiler_emits_RegisterMountHook`
5. `widget_owned_path_prefix_recorded_at_construction`
6. `flush_lifecycle_on_drain_removes_matching_unmount_hooks`
7. `flush_lifecycle_on_drain_removes_matching_effect_slots`
8. `cancel_exit_clears_candidate_unmounts_for_prefix`
9. `pending_queues_initially_empty_after_render`
10. `drain_pending_unmounts_runs_deepest_first`
11. `drain_pending_mounts_runs_parents_first`
12. `call_opcode_skips_path_insert_when_lifecycle_inactive`

All tests use the StateManager and runtime APIs directly —
no `.ogh` source involved.

### Cross-merge dependencies

- M0 must land before M1, M2, M3, M4 (they all use the
  plumbing).
- M0 has no upstream dependencies — it's pure additive
  internal infrastructure.

### Open questions resolved here

- **Audit OQ#9 — `owned_path_prefix` cost.** Decision: use
  `String` (not `Cow<'static, str>` or `Option<String>`).
  For UL's tree of ~5,000 widgets, the cost is ~5,000 short
  strings. Strings are deduplicated by Rust's allocator at
  this scale; switching to `Cow` adds branchiness for
  uncertain win. Profile at M5; revisit only if hot.

### What M0 does NOT include

- Any `.ogh` syntax for hooks (M1).
- Any `effect` semantics (M2).
- The `Portal` widget (M3).
- The focus stack (M4).
- VM handlers for the four opcodes (M1/M2).

### Validation gate

- All M0 tests pass.
- All Phase 1 tests still pass (no regressions).
- UL builds clean.
- Manual smoke: load a UL UI; verify nothing observable
  changed (lifecycle_active = false everywhere; no path
  marking happens).

---

## Merge 1 — `on_mount` / `on_unmount` in `.ogh`

### Goal

Ship the user-visible lifecycle hook surface for the two
non-effect kinds. After M1, authors can write `on_mount`
and `on_unmount` in `.ogh` and the runtime fires them.

### Deliverables

- Scanner: `on_mount`, `on_unmount` keywords. Two new
  `TokenType` variants.
- Parser: block-form expressions inside `fn` bodies. New
  AST nodes `Statement::OnMount(Block)` and
  `Statement::OnUnmount(Block)`.
- Compiler: emits `Closure(idx) ; RegisterMountHook(hook_id)`
  and `Closure(idx) ; RegisterUnmountHook(hook_id)`. Each
  hook body compiles as a sub-`FunctionProto` with no
  parameters.
- Compiler: assigns `hook_id` (1, 2, …) per source order
  within a function, per kind.
- VM: implements `RegisterMountHook` and `RegisterUnmountHook`
  opcode handlers per the design (mount = inline check;
  unmount = persistent map overwrite).
- VM: implements pre-layout hook draining
  (`pending_unmounts`).
- VM: implements post-layout hook draining
  (`pending_mounts`).
- LSP: `OnMount`, `OnUnmount` keyword highlighting in
  `semantic_tokens.rs:65`.
- LSP: `LifecycleHook { kind: HookKind }` hover variant in
  `hover.rs`.
- LSP: conditional-hook warning #4 from the design's
  diagnostics table.
- `SyntaxError::severity` field added (default `Error`);
  `with_warning()` builder.

### Implementation steps

1. **Scanner additions** (`scanner/mod.rs`,
   `scanner/token_type.rs`): `OnMount`, `OnUnmount` token
   types; keyword-table entries.
2. **Parser additions** (`parser/statement.rs`): parse
   `on_mount { ... }` and `on_unmount { ... }` as
   statements within `fn` bodies. Reject at top-level (not
   inside a `fn`) with a clear error.
3. **AST additions** (`parser/node.rs`): `Statement::OnMount(Block)`,
   `Statement::OnUnmount(Block)`.
4. **Compiler — body compilation** (`runtime/compiler.rs`):
   for each `Statement::OnMount(body)`, compile `body` as a
   sub-`FunctionProto` (zero parameters; captures upvalues
   from surrounding function). Emit `Closure(proto_idx)`
   followed by `RegisterMountHook(hook_id)`. Same for
   `OnUnmount` with `RegisterUnmountHook`.
5. **`hook_id` assignment**: per-function counter, separate
   for mount vs unmount. Reset on entering a new function
   compilation. Stored as a field on the function-compile
   state.
6. **VM — `RegisterMountHook` handler** (`runtime/vm.rs`):
   pop closure; check `previous_active_paths.contains(&path)`;
   if false, push `(path, hook_id, closure)` onto
   `pending_mounts`. Otherwise drop the closure.
7. **VM — `RegisterUnmountHook` handler**: pop closure;
   insert into `unmount_hooks` map, overwriting any prior
   entry at `(path, hook_id)`.
8. **Pre-layout drain implementation** in
   `Renderer::pre_layout_drain`: sort `pending_unmounts` by
   path length descending; for each, look up closure in
   `unmount_hooks`, call via `call_bytecode_closure`,
   remove from map, log-and-continue on error.
9. **Post-layout drain implementation** in
   `Renderer::post_layout_drain`: sort `pending_mounts` by
   path length ascending; for each, call closure, log-and-
   continue on error.
10. **`SyntaxError::severity` field** in
    `parser/syntax_error.rs`. Default constructor sets
    `Error`; `with_warning()` builder sets `Warning`.
    Update `lsp/server.rs:285` to map severity →
    `DiagnosticSeverity`.
11. **LSP semantic tokens** (`lsp/semantic_tokens.rs:65`):
    add `OnMount`, `OnUnmount` to keyword arm.
12. **LSP hover** (`lsp/hover.rs`): add `LifecycleHook`
    variant; resolve at cursor position over the keyword;
    render to markdown with the body-scope summary from
    the design doc.
13. **LSP conditional-hook warning**: parser walk that
    looks for `OnMount` / `OnUnmount` statements inside
    `Conditional`, `Match`, or `ForLoop` AST nodes; emit
    `SyntaxError` with warning severity. Trigger lives in
    parser (not strict-mode resolver) to keep diagnostics
    available without typed-bindings.
14. **Per-frame error log buffer** on `Runtime`:
    `lifecycle_error_log: Vec<String>` (capacity 100,
    drops oldest on overflow). Reset at frame start.
    `lifecycle_error_count() -> usize` API.
15. **Hook body error policy**: in
    `Renderer::pre_layout_drain` and `post_layout_drain`,
    wrap each closure call with `match`; on error, format
    to string, push onto `lifecycle_error_log`, increment
    counter, continue with next.

### Test matrix (~15 tests)

`tests/parser_lifecycle.rs` (new):

1. `parses_on_mount_block`
2. `parses_on_unmount_block`
3. `parses_multiple_on_mount_blocks_in_one_function`
4. `rejects_on_mount_at_module_top_level`
5. `warns_on_mount_inside_if_block`
6. `warns_on_unmount_inside_match_arm`

`tests/lifecycle_runtime.rs` (new):

7. `on_mount_fires_on_first_render`
8. `on_mount_does_not_fire_on_subsequent_renders`
9. `on_mount_fires_after_layout_can_read_post_layout_state`
10. `on_unmount_fires_when_path_disappears`
11. `on_unmount_fires_with_last_rendered_scope`
12. `on_unmount_delayed_until_drain_with_exit_animation`
13. `on_unmount_cancelled_by_cancel_exit_does_not_fire`
14. `multiple_unmount_hooks_in_one_function_all_fire`
15. `hook_body_error_logged_and_continues_to_next_hook`

`tests/lsp_lifecycle.rs` (new):

16. `hover_on_on_mount_keyword_returns_lifecycle_hook_info`
17. `semantic_tokens_highlight_on_mount_as_keyword`

(That's 17 — slightly over budget but the LSP tests are
small.)

### Cross-merge dependencies

- Depends on M0 (uses the plumbing).
- Does not block M3/M4 (Portal can ship with M1's hooks
  available for the focus-stack push/pop).

### Open questions resolved here

- **Audit OQ#1 — error log buffer shape.** Decision:
  ring buffer of `String` (not `RuntimeError`), capacity
  100. Stringification at log time keeps the API simple
  and survivable across runtime resets.
- **`hook_id` stability across edits.** Documented:
  source-order assignment means inserting a hook between
  existing ones shifts the later ones' IDs. Authors who
  care about identity-stability should structure
  hooks append-only or use a single block. M1 doesn't
  enforce; M5 documentation calls it out.

### What M1 does NOT include

- `effect` and `cleanup` (M2).
- Portal widget (M3).
- Focus stack (M4).
- Effect-related diagnostics (M2).

### Validation gate

- All M0 + M1 tests pass.
- UL builds clean.
- Manual smoke: write a tiny `on_mount { log "hi" }` in a
  test `.ogh`, verify it logs once on first render and not
  on subsequent renders.

---

## Merge 2 — `effect` with deps + `cleanup`

### Goal

Ship the third hook kind. Effects re-fire when deps change;
cleanup runs before re-fire and at unmount.

### Deliverables

- Scanner: `effect`, `cleanup` keywords.
- Parser: `effect (dep_a, dep_b) { body }` and
  `cleanup { body }` block forms. AST nodes
  `Statement::Effect { deps: Vec<Expr>, body: Block }`
  and `Statement::Cleanup(Block)`.
- Compiler: emits `[deps...] ; Closure(idx) ;
  RegisterEffect { hook_id, dep_count }` for `effect`.
- Compiler: emits `Closure(idx) ; RegisterEffectCleanup`
  for `cleanup` inside an effect body.
- Compiler: rejects `cleanup` outside an effect body
  (compile-time error #1 from diagnostics table).
- Compiler: validates dep expressions resolve to types
  with structural equality (rejects function refs etc. —
  diagnostic #2).
- VM: implements `RegisterEffect` opcode (dep comparison,
  fire-scheduling).
- VM: implements `RegisterEffectCleanup` opcode (attaches
  to current effect's slot).
- VM: drain `pending_effect_cleanups` in pre-layout step;
  drain `pending_effect_fires` in post-layout step.
- LSP: `Effect` keyword + `cleanup` keyword highlighting.
- LSP: `Effect { dep_count: usize }` hover variant;
  extended `Keyword` hover for `cleanup`.
- LSP: dep-type error #2 surfaced.
- LSP: conditional-`effect` warning #5.

### Implementation steps

1. **Scanner**: `Effect`, `Cleanup` token types.
2. **Parser**: parse `effect (deps) { body }` and
   `cleanup { body }`. The dep list is parens-delimited,
   comma-separated. Empty deps (`effect ()`) is legal.
   Reject `cleanup` outside an `effect` body during AST
   construction.
3. **AST**: `Statement::Effect { deps, body }` and
   `Statement::Cleanup(body)`.
4. **Compiler — effect body**: compile body as
   sub-`FunctionProto`. Emit each dep expression in
   order, then `Closure(proto_idx)`, then
   `RegisterEffect { hook_id, dep_count: deps.len() }`.
5. **Compiler — cleanup**: when compiling an effect body,
   track `current_effect_id` on the compile state. Emit
   `RegisterEffectCleanup` for `cleanup` blocks; the
   opcode handler uses the most-recently-fired effect's
   slot.
6. **Strict-mode resolver dep check**: walk effect deps;
   resolve each expression's type via the existing
   typed-bindings infrastructure; reject `Function`,
   `Mutation`, opaque host-state types with diagnostic #2.
   This integrates with the M2-from-Phase-1 strict-mode
   pipeline.
7. **VM — `RegisterEffect` handler**: pop
   `dep_count` values into `current_deps`. Pop closure.
   Look up `effects[(path, hook_id)]`:
   - If absent or `previous_deps.is_none()`: schedule
     fire (push onto `pending_effect_fires`); store new
     slot.
   - If present and `previous_deps != Some(current_deps)`:
     schedule cleanup (push prior `pending_cleanup` onto
     `pending_effect_cleanups`); schedule fire; store new
     `previous_deps` and `closure`; clear
     `pending_cleanup`.
   - If equal: re-store `closure` (refresh upvalues), no
     scheduling.
8. **VM — `RegisterEffectCleanup` handler**: pop closure;
   set `effects[(path, hook_id)].pending_cleanup =
   Some(closure)`. The "current effect" is tracked via a
   small VM-frame field set when an effect fire begins
   and cleared when it ends.
9. **Pre-layout drain — `pending_effect_cleanups`**:
   for each `(path, hook_id)`, call the cleanup closure,
   set slot's `pending_cleanup = None`, log-and-continue.
10. **Post-layout drain — `pending_effect_fires`**: for
    each `(path, hook_id)`, look up slot, call the
    closure, log-and-continue. The closure may register
    a new `pending_cleanup` via opcode 8.
11. **Drain order on unmount**: when a path unmounts
    (drained), `flush_lifecycle_on_drain` (from M0)
    schedules the slot's `pending_cleanup` onto
    `pending_effect_cleanups` for the next frame's
    pre-layout, then removes the slot. M0 is updated
    here to handle effect slots in addition to unmount
    hooks (was deferred from M0 since the slot type
    didn't exist yet).
12. **LSP**: `Effect`, `Cleanup` semantic tokens; hover
    variants; warning #5 (effect inside conditional).

### Test matrix (~12 tests)

`tests/parser_effects.rs` (new):

1. `parses_effect_with_single_dep`
2. `parses_effect_with_multiple_deps`
3. `parses_effect_with_empty_deps`
4. `parses_cleanup_inside_effect_body`
5. `rejects_cleanup_outside_effect`
6. `warns_effect_inside_if_block`

`tests/effect_runtime.rs` (new):

7. `effect_fires_once_on_first_render`
8. `effect_does_not_fire_when_deps_unchanged`
9. `effect_re_fires_when_dep_value_changes`
10. `effect_with_empty_deps_fires_only_once`
11. `cleanup_runs_before_effect_re_fires`
12. `cleanup_runs_when_path_unmounts`
13. `effect_with_function_dep_emits_compile_error`

`tests/lsp_effects.rs`:

14. `hover_on_effect_keyword_shows_dep_count`

### Cross-merge dependencies

- Depends on M0 + M1 (uses plumbing + closure machinery).
- Does not block M3/M4.

### Open questions resolved here

- **Audit OQ#2 — re-registration cost.** Confirmed
  acceptable for M2; profile during M5.
- **Audit OQ#3 — dep evaluation side effects.** Resolved
  by documentation only: "dep expressions should be pure
  reads." Adding a side-effect check is out of scope.

### What M2 does NOT include

- Portal (M3).
- Auto-tracked deps (signal-based; explicitly Phase 3+).
- React-style "if you forget a dep, we warn you" — Ogham
  doesn't have the AST traversal infrastructure to do
  that reliably without false positives. Defer.

### Validation gate

- All M0 + M1 + M2 tests pass.
- UL builds clean.
- Smoke: write `effect (state.x) { log state.x }` —
  toggle `state.x`, verify body re-fires only on change.

---

## Merge 3 — Portal: deferred-paint primitive

### Goal

Land the largest single mechanical change in Phase 2: the
two-pass paint pipeline + `Portal` widget. After M3,
portal contents render in front of their siblings; hit-test
finds them first; entry/exit animations work transparently.
Focus_trap is **not** in M3 (M4).

### Deliverables

- New `PortalWidget` type in `widget/portal.rs`. Three
  properties: `open: bool`, `focus_trap: bool` (parsed but
  unused in M3), `children: Vec<WidgetRef>`. Behaves like
  a no-op `Flex` in Pass A.
- `Portal` registered in `WidgetRegistry::with_defaults()`.
- Renderer field: `portal_layer: Vec<PortalEntry>`,
  cleared per frame.
- `draw_widget_recursive` learns to defer Portal children:
  push to `portal_layer`, do not recurse.
- Pass B: `paint_portal_layer` walks the layer, paints
  each portal's children with viewport clip + parent_rect
  origin.
- Hit-testing: `hit_test` searches portal_layer first
  (LIFO), falls through to base tree.
- Reconciliation: `Portal { open: false }` triggers exit
  animation on children via existing
  `begin_exit` / drain machinery.
- LSP: `Portal` is a built-in widget identifier; existing
  identifier-styling treatment applies. No special LSP
  work for the name.
- LSP: diagnostic #3 (Portal `children` type check) in
  the parser/typecheck pass.

### Implementation steps

1. **Define `PortalWidget`** in `widget/portal.rs`. Implements
   the existing `Widget` trait. Layout: zero-size box (Portal
   itself takes no layout space). Paint: pushes self to
   `portal_layer`, returns without recursing.
2. **Register in `WidgetRegistry::with_defaults()`** at
   `runtime/mod.rs:226`.
3. **`PortalEntry` struct** in renderer module:
   ```rust
   struct PortalEntry {
       widget: WidgetRef,
       parent_rect: Rect,
       focus_trap: bool, // unused in M3, parsed for M4
   }
   ```
4. **`portal_layer: Vec<PortalEntry>`** added to renderer
   state. Cleared at `paint_frame` start.
5. **Modify `draw_widget_recursive`** to branch on portal:
   ```rust
   if let Some(portal) = widget.as_portal() {
       if portal.is_open() {
           self.portal_layer.push(PortalEntry { ... });
       }
       return;  // do not recurse
   }
   ```
6. **Pass B implementation** — `paint_portal_layer(entry)`:
   set clip rect to viewport; set layout origin to
   `entry.parent_rect.top_left`; recurse into children
   normally.
7. **Layout for portal children** — runs as part of Pass B
   layout. Portal children's Flex layout uses parent_rect
   as their "parent" rect. Coordinate handling: the
   portal's `parent_rect` is captured during Pass A's
   layout pass; Pass B reuses it.
8. **Hit-test branch** in `hit_test`:
   ```rust
   for entry in self.portal_layer.iter().rev() {
       if let Some(hit) = hit_test_portal(entry, point) {
           return Some(hit);
       }
   }
   hit_test_recursive(root, point)
   ```
9. **Reconciliation through `open`**: when `open` flips
   `true → false`, the children (currently in the live
   widget tree as portal_layer entries) are reconciled
   out — call `begin_exit` on them. They become ghosts in
   the portal_layer; drain on next frame after springs
   settle.
10. **`PortalWidget::owned_path_prefix`** returns the
    portal's call_stack path, so M0's drain hooks fire
    cleanup for any state cells / hooks declared inside
    the portal's children.
11. **Diagnostic #3** in parser/typecheck: when
    constructing a `Portal { children: ... }` widget,
    verify the value is `Array<Widget>`. Reject with the
    designed error message otherwise.
12. **`Portal` hover** (`lsp/hover.rs`): add `Portal`
    variant; resolve at cursor over `Portal` identifier;
    render the API description.

### Test matrix (~10 tests)

`tests/portal_render.rs` (new):

1. `portal_with_open_true_renders_children_in_front`
2. `portal_with_open_false_does_not_render_children`
3. `portal_renders_in_front_of_overflow_hidden_parent`
4. `multiple_portals_stack_in_mount_order`
5. `portal_child_with_full_viewport_swallows_clicks`
6. `portal_hit_test_searches_portal_layer_before_base_tree`
7. `portal_open_false_to_true_runs_entry_animation`
8. `portal_open_true_to_false_runs_exit_animation`
9. `portal_owns_path_prefix_for_state_cleanup`
10. `portal_children_array_type_check_rejects_non_array`

### Cross-merge dependencies

- Depends on M0 (`owned_path_prefix`), M1 (Portal's
  internal lifecycle hooks for M4's focus stack — though
  M3 doesn't use them yet, the trait surface needs to be
  there).
- Does not block M5 (UL escape menu can wait for M4 to
  ship `focus_trap`, but tooltips can use M3 alone).

### Open questions resolved here

- **Audit OQ#3 — Portal open mid-exit-animation.**
  Verified by test #8 — `open: true → false → true` while
  exit is in flight cancels the exit and re-mounts.
- **Audit OQ#5 — Portal inside `Presence`.** Tested by
  ensuring the portal's `open` participates in normal
  reconciliation, which `Presence` already drives via
  generation key changes. Add to test matrix as test #11
  if room.

### What M3 does NOT include

- `focus_trap` semantics (M4) — the property is parsed
  and stored but doesn't affect focus.
- `has_input_blocking_portal` runtime API (M4).
- Focus stack on `UI` (M4).
- True z-index for non-portal widgets (out of scope).

### Validation gate

- All M0–M3 tests pass.
- UL builds clean.
- Smoke: write a tooltip Portal in a test `.ogh`; verify
  it paints in front of the parent's clip, dismisses on
  outside click, animates in/out.

---

## Merge 4 — Portal: `focus_trap` + `has_input_blocking_portal`

### Goal

Land focus isolation. After M4, modal portals trap focus,
last-opened wins, and consumers can derive their own
input-gating booleans from `has_input_blocking_portal()`.

### Deliverables

- `UI.focus_stack: Vec<FocusRestoration>` field added.
- `FocusRestoration` struct: `portal_path: String`,
  `previous_focus: Option<WidgetRef>`.
- `Portal`'s internal lifecycle hooks: on mount with
  `focus_trap: true`, push to focus stack; on unmount,
  pop and restore. (Uses M1's hook plumbing internally —
  not exposed to consumer's `.ogh`.)
- Focus-changing operations (tab, arrow keys,
  programmatic `set_focused`) consult `focus_stack.last()`
  and reject moves outside the trapped portal's subtree.
- New `Runtime::has_input_blocking_portal() -> bool` API.
- Hot-reload: `focus_stack` clears on runtime reset
  (per audit OQ#6 resolution).

### Implementation steps

1. **`FocusRestoration` struct** in `ui/mod.rs`.
2. **`UI.focus_stack`** field added; default empty.
3. **Portal's internal `on_mount`/`on_unmount`** wired in
   `PortalWidget::init`. The mount handler captures
   `UI.focused`, pushes to stack. Unmount restores. This
   uses M1's plumbing directly — `PortalWidget` constructs
   closures internally and registers them via the same
   opcodes.
4. **Focus-move check**: extract the existing
   focus-changing logic into a helper that consults the
   focus stack:
   ```rust
   fn try_set_focus(&mut self, target: WidgetRef) -> bool {
       if let Some(top) = self.focus_stack.last() {
           if !target.path().starts_with(&top.portal_path) {
               return false; // rejected
           }
       }
       self.focused = Some(target);
       true
   }
   ```
5. **`Runtime::has_input_blocking_portal`**: walks the
   renderer's `portal_layer` and returns `true` if any
   entry has `focus_trap = true`.
6. **Hot-reload reset**: `Runtime::clear_lifecycle_state`
   helper called from `set_module` / hot-reload paths.
   Clears `focus_stack`, `focused = None`, all pending
   queues, all hook registries.
7. **Multiple portals stacking** — verified by stack
   structure: `focus_stack.last()` always returns the
   most-recently-pushed trap.
8. **Edge case: trap with no focusable subtree**
   (audit OQ#10). Behavior: `try_set_focus` returns false
   for any target outside the portal subtree; if the
   portal subtree itself has no focusable widget, focus
   stays where it was at mount time (which `previous_focus`
   captured). Documented as designed behavior.

### Test matrix (~8 tests)

`tests/focus_trap.rs` (new):

1. `focus_trap_portal_pushes_to_focus_stack_on_mount`
2. `focus_trap_portal_restores_focus_on_unmount`
3. `set_focused_outside_trapped_portal_rejected`
4. `set_focused_inside_trapped_portal_accepted`
5. `nested_focus_trap_portals_stack_correctly`
6. `escape_from_nested_trap_returns_to_parent_trap`
7. `has_input_blocking_portal_true_when_modal_open`
8. `hot_reload_clears_focus_stack`

### Cross-merge dependencies

- Depends on M0, M1 (closure registration), M3 (Portal
  exists).
- Does not block M5.

### Open questions resolved here

- **Audit OQ#4 — nested focus-trap stacking.** Verified
  by tests #5, #6.
- **Audit OQ#6 — hot-reload focus stack.** Resolved:
  clear on reset.
- **Audit OQ#10 — focus_trap with no focusable subtree.**
  Documented as designed (focus stays at previous_focus).

### What M4 does NOT include

- Auto-focus behavior for trapped portals (consumer can
  programmatically set focus on mount; not a Portal
  policy).
- Focus visualization changes (existing focus-render
  paths still apply).
- Keyboard event routing changes — the focus stack only
  affects `try_set_focus`; key event delivery to focused
  widgets works unchanged.

### Validation gate

- All M0–M4 tests pass.
- UL builds clean.
- Smoke: write a modal Portal with two focusable buttons;
  verify tab cycles only between them; verify Escape
  doesn't escape (it's a consumer key handler concern).

---

## Merge 5 — UL validation pass + docs + worked examples

### Goal

Migrate the three M5 UL deliverables; ship the
`examples/portals/components.ogh` reference library;
graduate the design doc; resolve the remaining hot-reload
open question (audit OQ#7).

### Deliverables

- **Settings save-on-close** migrated:
  `actions.rs:2646`'s `client_settings.save()` moves
  behind a `save_settings` event handler; `settings.ogh`
  gets `on_unmount { event("save_settings", form) }`.
  `CloseSettings` action handler simplified.
- **Escape menu Portal migration**:
  `escape_menu.ogh` rewritten per the design doc's worked
  example. `overlay_state` plumbing in `update.rs`
  collapsed to `let overlay_active =
  ogham.has_input_blocking_portal()`.
  `confirm_disconnect` and `confirm_reset_run` bools moved
  from `mod.rs:1209–1210` into `state` cells in the
  `.ogh`. Nested confirm-disconnect becomes a nested
  Portal.
- **Inventory tooltip**: one tooltip on inventory cells
  showing item details on hover. Implemented as the
  worked non-modal Portal example.
- `examples/portals/components.ogh` ships with `Modal()`,
  `Tooltip()`, and `Dropdown()` library `fn`s.
- Hot-reload + lifecycle interaction tested and either
  works correctly or has a known-limitation
  documented (audit OQ#7).
- `LIFECYCLE_AND_PORTAL.md` status banner updated:
  "Live design contract" → "Live contract — Phase 2
  shipped."
- `LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md` updated with
  a per-merge "what shipped" section (mirrors Phase 1's
  doc graduation pattern).

### Implementation steps

1. **Settings migration**:
   - Add `save_settings` event handler in
     `actions.rs` (extract logic from
     `CloseSettings` handler).
   - Add `on_unmount { event("save_settings", form) }`
     to `settings.ogh`.
   - Simplify `CloseSettings` to just close the UI (no
     save).
   - Test: open Settings, change a value, close — verify
     save happens.
2. **Escape menu migration**:
   - Rewrite `escape_menu.ogh` to use `Portal { open:
     host_state.show_escape_menu, focus_trap: true,
     children: [...] }`. Backdrop becomes first child
     with `on_click` to dispatch hide event.
   - Move `confirm_disconnect`, `confirm_reset_run` from
     `mod.rs` to `state` cells in `escape_menu.ogh`.
   - Implement nested confirm-disconnect as nested
     `Portal { focus_trap: true, ... }`.
   - Replace `overlay_state` plumbing in
     `update.rs:347, 1469` with
     `ogham.has_input_blocking_portal()`.
   - Test: open menu, verify focus trapped, verify
     world input gated, verify confirm-disconnect nested
     trap works, verify Escape key dispatches hide event.
3. **Inventory tooltip**:
   - Add hover state to inventory cells.
   - Wrap details Flex in `Portal { open: state.hover,
     children: [tooltip Flex with translate_y transform] }`.
   - Test: hover cell, verify tooltip appears below.
4. **`examples/portals/components.ogh`**:
   - `Modal({ open, children })` — Portal with
     `focus_trap: true`, backdrop child, centered
     dialog wrapper.
   - `Tooltip({ open, text, anchor })` — Portal with
     transform-positioned text Flex.
   - `Dropdown({ open, items, on_select })` — Portal with
     anchor-relative item list, outside-click dismiss.
5. **Hot-reload + lifecycle test** (audit OQ#7):
   - Open the escape menu in a UL session.
   - Modify `escape_menu.ogh`, save.
   - Hot-reload triggers; observe whether pending
     unmounts flush correctly, whether the new module's
     mount fires, whether focus stack handles the swap.
   - If it works, document. If it doesn't, fix or
     document as known limitation.
6. **Doc graduation**:
   - Update `LIFECYCLE_AND_PORTAL.md` status banner.
   - Add a "What shipped" section to this doc with
     per-merge dates and notes.

### Test matrix (~5 integration tests)

`tests/ul_integration.rs` (or in UL's own test suite —
see open question below):

1. `settings_on_unmount_saves_via_event`
2. `escape_menu_portal_traps_focus`
3. `escape_menu_nested_confirm_traps_focus_separately`
4. `inventory_tooltip_appears_on_hover`
5. `has_input_blocking_portal_correct_during_escape_menu`

Plus hot-reload smoke test (manual, since the existing
hot-reload tests are integration-heavy).

### Cross-merge dependencies

- Depends on all of M0–M4.

### Open questions resolved here

- **Audit OQ#7 — drain-time unmount and module reload.**
  Resolved by M5 hot-reload smoke test outcome. Either
  works (most likely) or gets documented as a known
  limitation with a fix tracked separately.
- **Audit OQ#8 — Portal default registration.** Verified
  in M3 already; M5 confirms via UL adoption.

### What M5 does NOT include

- Migration of the other ~12 UL UIs (post-Phase-2
  backlog per audit).
- Editor UI migrations (deferred per audit).
- Async hooks (Phase 3+).
- True z-index (deferred).

### Validation gate

- All M0–M5 tests pass.
- UL builds clean.
- UL launches; escape menu works correctly with focus
  trap; settings save on close; tooltip appears.
- No regressions in any UL UI.
- Hot-reload tested manually for the escape menu case.
- `LIFECYCLE_AND_PORTAL.md` graduates.

---

## Cross-cutting concerns

### Documentation graduation

`LIFECYCLE_AND_PORTAL.md` is currently "Live design
contract — implementation pending." It graduates at M5
to "Live contract — Phase 2 shipped." This doc gains a
"What shipped" trailer at M5 (mirrors how
`TYPED_BINDINGS_IMPLEMENTATION.md` got per-merge dated
entries during Phase 1).

`LIFECYCLE_AND_PORTAL_UL_AUDIT.md` stays "Live audit"
indefinitely — the per-UI section is a living document
that gets updated as backlog items ship post-Phase-2.

### Test infrastructure for hook timing

Phase 2's tests need to be able to assert on hook firing
order. The existing test infra (run module, inspect state,
assert) is sufficient for most cases. For
order-sensitive tests, add a test helper in
`tests/common/mod.rs`:

```rust
pub fn collect_hook_log(runtime: &mut Runtime) -> Vec<String> {
    runtime.lifecycle_error_log_and_clear()
        .into_iter()
        .chain(runtime.lifecycle_fire_log_and_clear())
        .collect()
}
```

Plus a per-test "log when this hook fires" pattern — hooks
in test `.ogh` use `event("test_log", "mount fired")` and
the test handler appends to a shared log.

### Per-frame allocation budget

The audit raised concern about per-frame allocation in
heavy UIs. Phase 2 adds:

- One `String` allocation per `Call` opcode when
  `lifecycle_active = true` (the path key).
- One `Rc<VMClosure>` per `on_unmount` re-registration
  per frame.
- One `Vec<Value>` per `effect` per frame (the deps).

For a hookless UL frame: zero overhead.
For a heavily-hooked frame: ~100 small allocations.
Budget tested at M5; acceptance criterion is "no
observable frame-time regression."

### Hot reload contract

Phase 1 established hot-reload preserves typed-binding
schema state. Phase 2 needs to specify what happens to
lifecycle state across reloads:

- **Path-keyed state cells**: preserved (per Phase 1
  contract).
- **`unmount_hooks` and `effects` registries**: cleared.
  The new module re-registers them on next render.
- **`pending_*` queues**: flushed before reload (any
  pending unmounts fire; pending mounts are dropped).
- **`focus_stack`**: cleared (per audit OQ#6 / M4
  resolution).
- **Per-frame error log**: cleared.

This is documented in `LIFECYCLE_AND_PORTAL.md` §"What
M5 does NOT include" gets a hot-reload-contract callout.

### LSP error wording

The five new diagnostic messages need to be checked
once at M2 (when the warning channel goes live) by
reading them aloud as a UL author. Wording polish is
part of M2's gate.

---

## Risk register

Consolidated risks with mitigation per merge.

| # | Risk | Probability | Severity | Mitigation | Owner merge |
|---|---|---|---|---|---|
| 1 | `owned_path_prefix` allocation cost shows up in profile | Medium | Low | Switch to `Cow<'static, str>` if hot | M0 (initial), M5 (verify) |
| 2 | Drain-time unmount races with frame boundaries | Low | High | M0 unit tests cover drain ordering; M5 hot-reload test catches edge cases | M0, M5 |
| 3 | Closure scope capture for unmount captures wrong scope | Medium | High | Re-registration pattern means upvalues are always fresh; tested explicitly in M1 | M1 |
| 4 | Effect dep equality fails for nested records | Medium | Medium | `Value::eq` already handles records; add explicit test in M2 | M2 |
| 5 | Two-pass paint breaks existing renderer assumptions | High | High | M3 hardens against this with extensive tests; manual UL smoke before commit | M3 |
| 6 | Portal hit-test order causes input-routing bugs in UL | Medium | High | M3 tests + M5 UL escape-menu integration test cover the canonical case | M3, M5 |
| 7 | Focus stack semantics confusing to consumers | Medium | Medium | M5 escape menu nested-trap tests document the contract | M4, M5 |
| 8 | Conditional-hook warning fires falsely on edge cases | Medium | Low | Wording careful + warning-only (suppressible by author choice) | M1, M2 |
| 9 | Hot-reload + focus-trapped portal corrupts focus state | Medium | Medium | M4 + M5 explicit tests; clear focus stack on reload | M4, M5 |
| 10 | Per-frame allocation budget regresses UL frame time | Low | Medium | Profile during M5; acceptance criterion specifies no regression | M5 |
| 11 | LSP warning chatty for legitimate patterns | Medium | Low | Wording allows authors to suppress by restructuring; no syntax-level suppression in v1 | M1 |
| 12 | Editor UI migrations create scope creep | Medium | Low | Audit explicitly defers editor UIs; M5 sticks to the three named migrations | M5 |
| 13 | Settings tab-conditional setup tests the warning poorly | Low | Low | M5 deliberately migrates Settings to validate the warning fires correctly | M5 |
| 14 | Two-pass paint requires Skia clip-rect changes that ripple | Medium | Medium | Plan for one extra clip push/pop per portal; verify at M3 | M3 |
| 15 | Effect dep evaluation cost dominates at scale | Low | Low | Per audit OQ#2: defer optimization; revisit if profile shows hot path | M2, M5 |

### Risks not in the table

Some classes of risk are explicitly accepted:

- **No async**: documented limitation; consumers route through events. Not "mitigated," just "scoped out."
- **No true z-index**: same.
- **No hook composition**: same.
- **`on_unmount` is best-effort, not transactional**: documented in design; UL pattern (action handler does sync save) mitigates by not exclusively relying on `on_unmount`.

---

## Summary timeline

Estimated calendar time assuming ~1–2 person-days per
moderate merge, ~2–3 per high-risk:

| Merge | Estimate | Cumulative |
|---|---|---|
| M0 — Lifecycle plumbing | 2 days | 2 |
| M1 — `on_mount` / `on_unmount` | 2 days | 4 |
| M2 — `effect` + `cleanup` | 2 days | 6 |
| M3 — Portal: deferred-paint | 3 days | 9 |
| M4 — Portal: focus_trap | 1 day | 10 |
| M5 — UL validation + docs | 2 days | 12 |

**Total: ~12 person-days**, similar density to Phase 1's
~14 days. Spread over calendar time as the team's pace
allows; nothing requires consecutive days.

Per the single-branch workflow, each merge commits
straight to `main` after passing its gate. Push freely
between merges; no PRs for internal Phase 2 work.

---

## Decision points before starting

Items that need a decision before M0 begins. These are
small and can be made now or at M0 kickoff:

1. **Test file naming.** Phase 1 used
   `tests/typed_*.rs` for typed-bindings tests. Phase 2
   uses `tests/lifecycle_*.rs` and `tests/portal_*.rs`,
   plus `tests/effect_*.rs` for the third hook kind.
   Approve.
2. **`hook_id` numbering policy.** Decided: 1-indexed,
   per-function, per-kind, source-order. Approve.
3. **Path-prefix matching for drain.** Decided: simple
   `starts_with` on the joined path string. No fancier
   matching (no trailing-slash, no escaping). Documented
   limitation: a state cell at path `panel/x` would be
   "owned by" any widget with prefix `panel/`. Acceptable
   because path components are function names that can't
   contain `/` themselves.
4. **Lifecycle log capacity.** Default 100 entries; ring
   buffer drops oldest. Adjustable via runtime config?
   Skip for M1 — make it a constant; expose configurability
   only if a consumer needs it.
5. **Where to expose `lifecycle_error_count`.** On
   `Runtime` directly. Not on `TypedOgham` — typed
   handles wrap `Runtime` and inherit access via
   `&Runtime`.
6. **Test-only event handler convention.** For lifecycle
   tests, the convention is `event("test_log", string)`
   appending to a shared log. Document this in
   `tests/common/mod.rs`.
7. **`Portal` widget file location.** New file
   `src/widget/portal.rs`. Module declaration in
   `src/widget/mod.rs:3`.
8. **Focus stack persistence across module reloads.**
   Decided: cleared on reload. Approve.

If any of these need re-litigation, do it at M0 kickoff,
not in the middle of a merge.

---

## What "Phase 2 ships" looks like

When M5's gate passes:

- `on_mount`, `on_unmount`, `effect`, `cleanup` are
  scanner keywords; the parser accepts the four block
  forms; the compiler emits the four new opcodes; the VM
  fires hooks at the documented timing.
- `Portal` is a built-in widget; its three properties
  parse correctly; it renders in front of base-tree
  siblings; hit-test finds portal contents first;
  `focus_trap: true` traps focus to the portal subtree.
- `Runtime::has_input_blocking_portal()` returns `true`
  when any open portal has `focus_trap: true`.
- `Runtime::lifecycle_error_count()` and
  `lifecycle_error_log()` exist for debug tooling.
- LSP highlights the new keywords; hovers on hook keywords
  return operational descriptions; warns on conditional
  hooks; rejects malformed effect deps and Portal children.
- UL's escape menu uses Portal with focus_trap; the
  `overlay_state` plumbing in `update.rs` is one line
  derived from `has_input_blocking_portal`; the nested
  confirm-disconnect is its own nested Portal; entry/exit
  animations on the dialog work correctly.
- UL's settings UI saves on close via `on_unmount` +
  event dispatch; the `CloseSettings` action handler is
  simplified.
- UL's inventory HUD has a tooltip on cells (one worked
  example).
- `examples/portals/components.ogh` ships with `Modal()`,
  `Tooltip()`, and `Dropdown()` library `fn`s.
- All Phase 1 functionality continues to work unchanged.
- Hot reload behaves correctly across the new state (or
  has known-limitations documented).
- `LIFECYCLE_AND_PORTAL.md` status banner reads "Live
  contract — Phase 2 shipped"; this doc has a per-merge
  "What shipped" trailer with dates and any deviations
  from plan.
- ~62 new tests pass.
- Post-Phase-2 backlog is recorded in project tracking,
  not in the live contract docs.

That's Phase 2 done.

---

## What shipped — per-merge trailer

Per-merge dates, LOC, and any deviations from the original
plan. All commits on `main` per the single-branch convention.

### M0 — Lifecycle plumbing — 2026-05-05 (`8c6c329`, +502 LOC)

**Shipped:** All StateManager extensions; four new opcodes
declared with stack effects (VM dispatch returns
InvalidOperation pending M1/M2); `owned_path_prefix` on
FlexWidget; `lifecycle_active` flag with bytecode scan +
Call-opcode gate; rotate_active_paths in execute_module +
rerender; flush_for_path_prefix + cancel_unmount_for_prefix
helpers; 12 plumbing tests.

**Deviations:**
- The `drain_exited_children` / `cancel_exit` wiring was
  deferred to M1 (was M0 step 7-8 in the plan). Rationale:
  threading `&mut StateManager` through `tick_animations`
  is invasive surgery, and M0 has no hooks registered to
  flush. M1 took over the wiring.
- Pre/post-layout drain stubs were similarly deferred (M0
  step 13). M1 implemented them as actual drain methods.

**Bug found and fixed in M0 follow-up (`357ceea`):**
Path-capture timing — `create_flex_widget` read
`runtime.state.call_stack` at builder time, but the
builder runs after `execute_module` returns by which point
call_stack is empty. Fix: capture the path in
WidgetDescriptor at Widget-opcode time. Regression test
added.

### M1 — `on_mount` / `on_unmount` — 2026-05-05 (`256316c`, +1043 LOC)

**Shipped:** Scanner OnMount/OnUnmount tokens; parser
Statement::OnMount/OnUnmount AST nodes; SyntaxError
severity field + with_warning + is_blocking; compiler
emits Closure + RegisterMountHook/RegisterUnmountHook
with per-fn hook_id counter; VM RegisterMountHook (queues
when newly mounted, drops otherwise) + RegisterUnmountHook
(persistent map overwrite); pre/post-layout drain pipeline
+ lifecycle_error_log + count/log APIs;
queue_disappeared_unmounts; LSP keyword highlighting +
LifecycleHook hover variant + conditional-hook walker; 15
tests.

**Deviations:**
- M1 ships **path-disappear unmount semantics**, not
  full drain-time. Unmount fires when the path stops
  being visited, not after the widget's exit animation
  completes. Cancel-mid-exit causes spurious unmount +
  remount. Rationale: full drain-time requires threading
  `&mut StateManager` through `tick_animations` (the M0-
  deferred wiring); deferred again to a future merge.
  M3 + M4 don't depend on drain-time semantics. The M5
  canonical migrations don't exercise cancel-mid-exit.
- Mount + unmount drain happens **inside `rerender()`**
  rather than around the host's `layout()` call.
  Rationale: avoids requiring host frame-loop changes.
  Mount fires before layout, slightly off the design's
  "after layout" ordering. M3+ refines if Portal needs
  post-layout sizes.
- Pending queue shape changed from `Vec<(String, u16)>`
  to `Vec<(String, u16, Rc<VMClosure>)>` (M0 had a bug
  where flush removed entries from the persistent map and
  pushed only keys, leaving the drainer with nothing).

### M2 — `effect` + `cleanup` — 2026-05-05 (`7345656`, +715 LOC)

**Shipped:** Scanner Effect/Cleanup tokens; parser
Statement::Effect/Cleanup AST nodes; compiler
compile_effect (deps + body sub-FunctionProto + Closure +
RegisterEffect) + compile_cleanup_inside_effect (Closure
+ RegisterEffectCleanup); per-fn next_effect_hook_id
counter; in_effect_body context flag; VM RegisterEffect
(deps comparison via Value::eq, schedule cleanup-then-fire
on change) + RegisterEffectCleanup (attach via
current_firing_effect side channel); post_layout_drain
fires effects (was a stub in M1); LSP keyword highlighting
+ Effect hover variant + conditional-effect warning #5;
13 tests.

**Deviations:**
- Effect dep type checking (design diagnostic #2) was
  initially missed and shipped in the audit follow-up
  (`565c889`). See audit entry below.

### Audit follow-up — 2026-05-05 (`565c889`, +449 LOC)

Pre-M3 audit found 3 bugs and added 5 behavior-locking
tests:

1. **Path prefix-matching false positive**:
   `"fn@10".starts_with("fn@1")` returned true byte-wise.
   Fix: `path_matches_prefix` helper requires exact match
   or `prefix + "/"` boundary. Affected
   `flush_for_path_prefix` and `cancel_unmount_for_prefix`.
2. **LSP conditional-hook walker never descended into fn
   bodies**: bailed at `Statement::Declare(_)` via the
   catchall arm. Fix: descend into `Function` literal
   bodies. The warning now actually fires in real .ogh
   code.
3. **Effect dep type check missing** (M2 deliverable
   skipped). Fix: `fn_typed_locals` HashSet on the
   Compiler, populated from `Declare` statements with
   `Function` literal RHS, inherited by child compilers.
   `compile_effect.check_dep_type` rejects fn literals +
   identifiers resolving to fn-typed lets.

Plus tests: multiple-cleanups-only-last-wins,
multiple-effects-fire-in-source-order, prefix-numeric-
sibling regression + 8-case path_matches_prefix coverage,
3 LSP integration tests for the descent fix, 3 effect
dep-rejection tests.

### M3 — Portal: deferred-paint primitive — 2026-05-05 (`d5212f4`, +846 LOC)

**Shipped:** PortalWidget (open + focus_trap + children +
owned_path_prefix); Widget::as_portal trait method;
UI.portal_layer + PortalEntry; Skia draw clears + walks
into portal_layer in main pass + paints Pass B with
parent_rect translation; hit-test searches portal_layer
first (LIFO via reverse iteration); builder
create_portal_widget with type-check rejection (design
diagnostic #3); registered in
WidgetRegistry::with_defaults; 10 tests.

**Deviations:**
- Two-pass paint correctness at the actual Skia surface
  level isn't validated by integration tests (no Skia
  surface in tests/). Was to be validated at M5 via the
  UL escape-menu integration; that migration deferred
  per M5 notes.
- A focus_trap portal without a backdrop child currently
  leaks unhandled clicks to the base tree. Documented
  M3 limitation; M4 wires focus_trap to gate this — but
  see M4 deviations.

### M4 — focus_trap + has_input_blocking_portal — 2026-05-05 (`ad4282e`, +346 LOC)

**Shipped:** UI.focus_stack + FocusRestoration;
sync_focus_stack (called from Skia's draw after Pass B);
try_set_focus rejects out-of-subtree moves;
clear_lifecycle_state for hot-reload safety;
Ogham::has_input_blocking_portal public API forwarding to
UI.has_input_blocking_portal; 9 tests.

**Deviations:**
- focus_stack reconciliation is **derived from per-frame
  portal_layer** rather than driven by Portal-internal
  lifecycle hooks (the original plan). Rationale:
  Portal-internal hooks would require threading the
  M1/M2 hook plumbing into PortalWidget's Rust code,
  which is invasive. The derived approach is simpler and
  has the same observable behavior.
- M4 try_set_focus is the focus-move gate; **key event
  routing is unchanged**. Trapped portals still receive
  keys via their own focused descendants; key events to
  non-focused widgets in the base tree pass through
  unchanged. Matches the design — focus_trap is about
  focus isolation, not key-event interception.
- Click-leak through a backdrop-less focus_trap portal
  (M3 limitation) is **not** wired in M4. Backdrops are
  the canonical solution, encoded in the
  `examples/portals/components.ogh` Modal helper.

### M5 — Examples library + doc graduation — 2026-05-05 (this commit)

**Shipped:** `examples/portals/components.ogh` with
Modal/Tooltip/Dropdown reference fns; design doc status
banner graduated to "Live contract — Phase 2 shipped";
this trailer added.

**Deviations (significant):**
- The three UL migrations (Settings save-on-close,
  escape menu Portal, inventory tooltip) are
  **deferred to a post-Phase-2 UL backlog**. Two
  reasons:
  1. UL's `overlay_state` pattern swaps Ogham instances
     when an overlay opens/closes. Runtime drop doesn't
     fire `on_unmount` mid-render — the path simply
     vanishes when the runtime is dropped. The M5
     Settings save migration would need UL to restructure
     so Settings stays inside the same Ogham instance,
     reaching `on_unmount` via path-disappear semantics.
     That's beyond the M5 surface — it's a UL refactor.
  2. UL has uncommitted in-progress combat work touching
     `client/mod.rs`, `update.rs`, and several ui/*.rs
     files. The M5 migrations would touch the same
     files. Cross-repo WIP collision risk is high.
- The patterns are demonstrated in
  `examples/portals/components.ogh` instead. UL adoption
  is now an ordinary post-Phase-2 task, joining the ~12
  person-day backlog from the audit's per-UI verdicts.
- Hot-reload + lifecycle smoke test (audit OQ#7) is also
  deferred — without a UL runtime exercising the
  hot-reload path with focus_trap portals open, there's
  nothing to smoke-test. Hot-reload + focus_stack reset
  is unit-tested via `clear_lifecycle_state`.

### Phase 2 totals

- **Implementation:** 4,247 LOC across M0–M5 + audit fix.
  Within the 2,500–3,500 LOC estimate's upper-bound
  range; bug fixes + audit additions account for the
  overage.
- **Tests:** 64 new tests (12 plumbing + 15 lifecycle +
  18 effect + 10 portal + 9 focus_trap, plus the audit's
  10 LSP/regression tests). Slightly above the impl
  plan's ~62 estimate.
- **Calendar:** all merges shipped on 2026-05-05 in
  one autonomous push. Plan estimated 12 person-days.
- **UL adoption:** 0 of 3 M5 migrations shipped.
  Deferred to post-Phase-2 backlog.

### What remains for the team

1. Post-Phase-2 UL adoption per the audit's per-UI
   table (~12 person-days). Top priority: Settings
   save-on-close (smallest), escape menu Portal
   (highest leverage), inventory tooltip (worked
   non-modal example).
2. Refine M1's path-disappear unmount semantics to
   full drain-time when needed (likely when authoring
   a portal that animates out — the cancel-mid-exit
   spurious-unmount edge case will surface).
3. Refine M1's mount timing if Portal positioning
   needs post-layout sizes.
4. Address the schema-diagnostics workstream's
   lib-test breakage (untracked
   `src/diagnostics/manifest.rs` uses derive macros
   that don't resolve `::ogham` paths inside the
   ogham crate itself — separate workstream).
