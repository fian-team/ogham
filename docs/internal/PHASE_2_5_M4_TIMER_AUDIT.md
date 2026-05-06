# P25-M4 — Timer primitive audit

> **Status: Audit result — no timer primitive shipped.**
> Companion to `PHASE_2_5_M4_PLAN.md`'s "audit + ship-if-
> missing" framing. The audit ran; no script-callable
> set_timeout/set_interval primitive exists in ogham today;
> no immediate UL-adoption use case requires one. Documented
> here so the next agent picking up tooltips/toasts has a
> clear path.

---

## Findings

**Existing scheduling infrastructure** in ogham:
- `tick_animations(dt)` per-frame walk via the Widget trait
  (`src/widget/mod.rs:980`). Drives spring interpolation for
  styling transitions (opacity, transform, color).
- Spring math in `src/widget/animation.rs` — stiffness +
  damping + per-frame integration. Not script-callable; no
  user-facing timer concept.
- Phase 2 M2 effect cleanup mechanism — but those fire on
  state changes, not delays.

**Existing scheduling in UL** (per UI_RUNTIME.md §5 prediction):
- `panel_transition` in `main.ogh:10` is a spring config
  object (`{ opacity: fast_spring, transform: fast_spring, ... }`),
  NOT a timer. The phase plan's hint that this implied a
  timer primitive was incorrect.
- No other timer-like patterns surfaced in the audit.

**Conclusion**: ogham has no script-callable timer primitive.
UL doesn't currently use one (none of the per-UI patterns in
`UL_ADOPTION_READINESS.md` §4 require it).

---

## What needs to ship — when?

UL_RUNTIME §5 lists timer use cases UL anticipates:
- Tooltip show-delay (hover for N ms before tooltip appears)
- Toast TTLs (auto-dismiss after N seconds)
- Animation sequences (chain transitions over time)
- Debounced input handlers

**Adoption sequencing**: none of these gate the UL Pass 2
canonical migrations (Settings save-on-close, escape menu
Portal, inventory tooltip-without-delay). The first real
consumer is **toast queues** (UI_SHELL §B2 mentions but
UL hasn't implemented). When that work starts, ship the
timer primitive then.

---

## Design sketch (for the future implementer)

When the timer primitive lands, recommend this shape per
UI_RUNTIME §5:

### API

```ogh
let timer_id = set_timeout(1000, fn () { event("timer_fired"); });
clear_timeout(timer_id);

let interval_id = set_interval(500, fn () { event("tick"); });
clear_interval(interval_id);
```

### Implementation skeleton

**Runtime side** (`src/runtime/mod.rs`):
- `Runtime::timers: HashMap<TimerId, TimerEntry>` where
  `TimerEntry { deadline_ms, callback: Rc<VMClosure>,
  owning_path: String, kind: Once | Repeating(period_ms) }`.
- `Runtime::next_timer_id: u64` (increments).
- `Runtime::tick_timers(dt)` — advance all deadlines; for
  each deadline ≤ 0, queue callback for next post-layout
  drain (reuse Phase 2 M2 effect-fire dispatch shape).
  Repeating timers reset their deadline to `period_ms`.
- Auto-cancel-on-unmount: when `flush_for_path_prefix` runs,
  walk timers and drop any whose `owning_path` matches the
  prefix. Pairs with Phase 2 M2's effect-cleanup machinery.

**Built-ins** (`src/runtime/vm.rs` or builtins module):
- `set_timeout(delay_ms, callback) -> TimerId` — pop args,
  insert into registry with current path, return `Value::Integer(id)`.
  (Or `Value::TimerHandle(id)` if a new variant feels worth it
  — analogous to Phase 2 M2's `Value::WidgetRef`.)
- `set_interval(period_ms, callback) -> TimerId` — same
  shape, kind = Repeating.
- `clear_timeout(id)` / `clear_interval(id)` — pop id,
  remove from registry.

**Frame integration**:
- Hosts call `runtime.tick_timers(dt)` once per frame
  (alongside `tick_animations`). Could fold into the existing
  tick_animations call.

**Tests** (~5):
- set_timeout fires after delay
- clear_timeout prevents fire
- set_interval fires repeatedly
- timer auto-cancels on widget unmount
- timer handle round-trips through Value type

**Estimated**: ~150-200 LOC + ~5 tests, ~1 person-day. Same
shape as the original M4 budget — just deferred to its actual
need.

---

## Why deferring is the right call

1. **No current consumer.** Building infrastructure ahead of
   need risks getting the API shape wrong — better to design
   when the first concrete use case (toast TTLs, tooltip
   delays) clarifies the requirements.
2. **Auto-cancel-on-unmount integration depends on Phase 2's
   path-keyed lifecycle.** That's stable now; the integration
   point is well-understood. No risk of having to re-do it
   later if Phase 2 lifecycle changes.
3. **Timers + drag-events Phase 3 might both want a
   shared "per-frame side effect" infrastructure.** Designing
   them together (when both are concretely needed) avoids
   premature abstraction.
4. **UL adoption path is clearer with the audit result
   documented than with a half-implemented stub.** Future
   agents can read this doc and know: timer doesn't exist
   yet; here's the design when it's needed.

---

## Updates to other docs

- `UL_ADOPTION_READINESS.md` §2.5 should be updated to mark
  "timer primitive: deferred per audit; design captured in
  PHASE_2_5_M4_TIMER_AUDIT.md."
- `PHASE_2_5_IMPLEMENTATION.md` M4 section trailer: this
  audit result + design sketch.
- The Ogham skill `SKILL.md` doesn't currently mention
  timers; nothing to update there.

---

## P25-M4 validation gate

- [x] Existing scheduling infrastructure surveyed.
- [x] No script-callable timer primitive found.
- [x] UL adoption path doesn't require one for canonical
      migrations.
- [x] Design sketch captured for future implementer.
- [x] Documentation references updated.
- [x] No code change → workspace still builds clean (no-op).
