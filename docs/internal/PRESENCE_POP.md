# Ogham — Presence `mode: pop`: overlapped generation transitions

> **Status: complete.** Drafted and shipped 2026-08-12 — M0/M1/M3
> landed together (mode plumbing, pop machinery, and pop-time prefix
> flush are one coherent change), M2 (global hit-test invisibility for
> exiting widgets) as its own commit, M4 docs alongside. One §5 detail
> refined during implementation: a ghost's *size* is re-derived from
> its style under the frozen constraints each pass (a `Grow` ghost
> resolves to the frozen rect; a `Shrink` ghost re-measures its
> content), rather than being hard-pinned — that is what lets interior
> layout-affecting exits reflow. Position is frozen as designed. Where
> the shipped code and this plan differ, the code is authority.
>
> Adds a `mode` field to the `Presence` widget with two values: `pop`
> (new default) and `wait` (the current exit-then-mount behaviour,
> retained for deliberately sequenced choreography). No language, VM,
> or reconciler changes; one behavioural change to global hit-testing
> (§6) that is arguably a bug fix in its own right.
>
> Design provenance: examined against the React ecosystem's
> `AnimatePresence` modes (`sync` / `wait` / `popLayout`),
> react-transition-group's `SwitchTransition`, and the View Transitions
> API. `pop` is Framer Motion's `popLayout` transplanted onto ogham's
> layout model, with **live ghosts, not snapshots** — a snapshot
> flattens the exiting subtree to a texture and caps the exit
> vocabulary at whole-image transforms; live ghosts keep per-descendant
> choreography (exit staggers, layout-affecting exits inside the frozen
> rect) and liveness (spinners keep spinning).

---

## 1. The problem with `wait`

`PresenceWidget` today is strictly serial
(`src/widget/presence_widget.rs`): on a `key` change the outgoing
generation gets `begin_exit()`, the incoming generation is staged as
`pending_children`, and `commit_pending()` runs only when
`inner.children` has drained to empty (`tick_animations`). The
perceived latency is worse than exit + entry, because three costs
stack:

1. **Spring settle tails.** A ghost drains on `is_exit_complete()`,
   which requires numerical settle (`SETTLE_POS = 0.01`,
   `src/widget/animation.rs:16`) — well past the point the exit is
   visually done.
2. **Slowest-descendant gating × exit staggers.** One exiting
   descendant anywhere holds the whole generation
   (`flex_widget.rs:1945`), and `begin_exit`'s cascade adds
   `slot × exit_step` group delay per child (`flex_widget.rs:1895`),
   so the *last* staggered child's delayed start + duration + settle
   tail gates the mount. The new generation then replays its own
   entry stagger on top — all serialized.
3. **A frame-boundary hop** between the last drain and the pending
   subtree's first laid-out frame.

What `wait` buys — exclusive generations never coexist in flow layout,
so no stacking or layout shift — `pop` buys differently: the exiting
generation is removed from *layout* instead of from *time*.

## 2. Semantics of `pop`

On a `key` change:

1. Every current child is **popped**: `begin_exit()` as today, but
   instead of holding the newcomer, the child is moved out of
   `inner.children` into a separate `ghosts` vec, pinned at its last
   layout rect.
2. The incoming generation mounts **immediately** into
   `inner.children` and plays its entry animations as a normal fresh
   mount. `generation_key` advances at the same moment.
3. Ghosts keep ticking their exit springs, paint **above** the live
   children, receive **no** input, and are dropped individually as
   each settles. No relayout on ghost drain — they were already out
   of flow.

There is no `pending` state in pop mode, which collapses the interrupt
matrix:

- **Rapid key changes** — each change pops the current generation onto
  the ghost pile and mounts the newcomer. Multiple ghost cohorts may
  coexist under fast switching (same as Framer's `sync`/`popLayout`);
  the pile self-drains.
- **Revert to a prior key** — not a special case. The old generation's
  ghost is already dying and stays dying; a fresh subtree for the
  reverted key mounts. `cancel_pending` / `cancelled_unmount_prefixes`
  have no pop-mode analogue.

Decisions locked with the author (2026-08-12): separate ghost vec
(§4); no repositioning of ghosts on window resize; ghosts paint above
live children; ghosts are hit-test-invisible; **`pop` is the
default** because it is the desired behaviour for nearly all uses.

## 3. The `mode` field

```text
Presence {
  key: current_page_id,
  mode: "wait",          // optional; default "pop"
  children: [ route_body() ]
}
```

- `PresenceMode { Pop, Wait }` on `PresenceWidget`, default `Pop`.
- Builder (`create_presence_widget`, `src/widget/builder.rs:992`)
  parses `mode:` — `"wait"` → `Wait`, `"pop"` or absent → `Pop`,
  following existing prop-parsing conventions for unknown values.
- `update()` adopts the incoming widget's mode alongside the style
  adoption, so hot reload picks up mode edits. A mode change applies
  to *future* transitions: an in-flight `wait` transition (staged
  `pending_children`) completes under wait rules; existing pop ghosts
  drain regardless of mode.

**This changes shipped behaviour**: existing UL apps using `Presence`
get overlapped transitions after upgrading unless they opt into
`mode: "wait"`. Deliberate — recorded here so the changelog says it
loudly.

## 4. Data model and freezing

```rust
struct Ghost {
    widget: WidgetRef,
    /// Layout rect at pop time, parent-relative (= Presence content
    /// space, same space inner children lay out in).
    rect: Rect,
}
// PresenceWidget gains:
ghosts: Vec<Ghost>,
mode: PresenceMode,
```

A **separate vec, not `inner.children`**, for one load-bearing reason:
`reconcile_children`'s pre-pass cancels any exiting ghost whose key
matches an incoming child (`flex_widget.rs:369`). Across a generation
swap keys frequently repeat (a route body keyed `"content"` in both
generations) — in-`children` ghosts would get their exit cancelled and
be *adopted* as the new child. The separate vec sidesteps the identity
collision, keeps `reconcile_children` untouched, and keeps ghosts out
of `inner.handle_event`'s child walks (both pointer and keyboard) for
free.

**Freezing is mostly automatic.** Layout rects are parent-relative
(see the coordinate comment at `flex_widget.rs:1125`), and a widget's
rect only changes when `layout()` is called on it. Once popped out of
`inner.children`, the flex pass no longer reaches the ghost, so its
rect — captured into `Ghost.rect` at pop time — stays put. Because the
rect is Presence-relative, ghosts move *with* the Presence (scrolling
ancestors, window-level reflow of the Presence itself); only their
slot within the Presence is frozen. A resize mid-exit can leave a
ghost misaligned relative to reflowed siblings for a few hundred ms;
accepted (same artifact as Framer's `popLayout`).

**Edge:** a child popped before its first layout pass has no rect.
Drop it immediately (drained-prefix path, §7) instead of ghosting at a
made-up position; a pre-first-frame swap is invisible anyway.

## 5. Per-hook mechanics

**`update()` — pop path** (key changed, mode `Pop`):

```text
for child in take(inner.children):
    rect   = child.get_layout_rect()
    can    = child.begin_exit()          // unchanged cascade + exit stagger
    prefix = child.owned_path_prefix()
    if can && rect.is_some():  ghosts.push(Ghost { child, rect })
    else if !prefix.is_empty(): result.drained_path_prefixes.push(prefix)
    // §7: prefixes of ghosted children are ALSO pushed here — flush at
    // pop, not at drain.
inner.children = new_children            // mounts now, entry springs armed
generation_key = new_key
```

Same-key updates reconcile through `inner.reconcile_children` exactly
as today; the ghost vec is not consulted (new children never match
against ghosts — that is the point of §4).

**`tick_animations()`** — after `inner.tick_animations(ctx)`:
tick every ghost; then `retain` those whose `is_exit_complete()` is
false. Drains request repaint but **not** relayout (out of flow), and
push **no** prefixes (§7). The wait-mode commit check
(`pending_children.is_some() && inner.children.is_empty()`) remains,
gated on mode `Wait`.

**`layout()`** — after `inner.layout(...)`: for each ghost,

```text
ghost.widget.layout(ctx, rect.x, rect.y, inner.direction,
                    rect.w, rect.w, rect.h, rect.h, 0.0)
```

Cursor = frozen origin; parent dims and available dims = frozen size,
`sibling_basis = 0`, so a `Grow` ghost resolves to exactly its frozen
rect and `Shrink`/fixed ghosts size as they did in flow. This per-pass
call is what keeps **interior** layout-affecting exit animations
(height → 0 inside the frozen box) actually reflowing; the frozen rect
itself never moves.

**`get_children()` / `get_children_mut()`** — return
`inner children ++ ghost widgets`, ghosts last. The render walk is
external (`draw_widget_recursive`, `src/skia.rs:1235`) and paints in
child order, so ghosts-last = ghosts-on-top, which is the agreed
z-order (matches View Transitions' old-over-new). This also keeps
ghosts visible to the portal-collection passes (`skia.rs:1146`) — a
portal inside an exiting subtree keeps painting while it dies, which
is the behaviour in-`children` ghosts have today.

**Exit-lifecycle delegation** (Presence being removed by *its*
parent):

- `begin_exit()`: drop `pending_children` (wait), keep ghosts (they
  are already exiting and visible); return
  `inner.begin_exit() || !ghosts.is_empty()`.
- `is_exiting()`: `inner.is_exiting() || !ghosts.is_empty()`.
- `is_exit_complete()`:
  `(!inner.is_exiting() || inner.is_exit_complete()) &&
  ghosts.all(is_exit_complete)` — the inner guard matters because
  `FlexWidget::is_exit_complete` returns `false` when not exiting, and
  a Presence can be a ghost purely on account of its ghost pile.
- `cancel_exit()`: `inner.cancel_exit()` only. Ghosts are from a dead
  generation; they have no future to return to.
- `restart_entry_animation()`: delegate to inner as today; ghosts keep
  dying naturally. (A re-promoted screen may briefly show a stale
  ghost finishing its fade — harmless, and dropping them here would
  bypass the drain bookkeeping.)

## 6. Hit-test invisibility of exiting widgets

Ghosts must not eat clicks, hovers, or drops aimed at the live content
underneath them. Presence-side event *dispatch* excludes them for free
(§4), but the external walks operate on `get_children()` and need a
skip. Make it a **global invariant — an exiting widget is
hit-test-invisible** — rather than a Presence special case:

- `UI::update_hover_recursive` (`src/widget/mod.rs:1088`) — treat
  `is_exiting()` widgets (and their subtrees) as not hit: clear
  hover, fire `mouse_leave` if needed, don't recurse for hover
  purposes.
- `UI::deepest_at` (`mod.rs:1018`) and `UI::deepest_drop_target`
  (`mod.rs:1047`) — skip exiting children.
- `FlexWidget::handle_event` pointer branch (`flex_widget.rs:1150`) —
  skip exiting children in the walk.
- `blocks_point` — same skip in its recursion (verify implementation
  sites).

Note this deliberately *changes* behaviour for today's in-`children`
reconcile ghosts, which currently can consume pointer events while
fading out. That is a bug by any reasonable reading — a half-faded
button should not be clickable — and fixing it globally is what makes
pop-mode overlap safe. Keyboard/focus routing to exiting subtrees is
left as-is (pre-existing question, out of scope; see §9).

## 7. Lifecycle prefixes: flush at pop, not at drain

The hazard: owned paths are call-stack paths with per-site call
counters (`StateManager::get_call_stack_path`,
`src/runtime/mod.rs:74`), so two generations that invoke the *same
component function at the same call site* (e.g. `detail_page(id)` for
two different ids) own the **same prefix**. In pop mode the new
generation mounts and registers state/hooks immediately; if the
ghost's prefix were flushed at drain time (the status-quo mechanism,
~300 ms later), it would clobber the live subtree's freshly-registered
state.

Resolution: **pop mode pushes the outgoing generation's prefixes into
`UpdateResult.drained_path_prefixes` at pop time** — the same channel,
same render-boundary timing, that the immediate-drop path
(exit-incapable children) uses today, whose drop-and-remount-same-path
ordering the host already handles. Ghost drains then push nothing.

Semantics: **logical unmount happens at replacement; visual death
later.** Unmount hooks and effect teardown for the old route fire when
the new route takes over, not when its pixels finish fading — which is
also the desirable behaviour (an old route's timers stop immediately).
Ghosts are pure widget-side spring animation from pop onward and need
nothing from the runtime.

One wrinkle to preserve: nested flexes *inside* a ghost still drain
their own exited children through `drain_exited_children`, pushing
sub-prefixes of the already-flushed generation prefix. Overlapping /
duplicate prefixes already occur today (a ghost root's prefix covers
sub-prefixes its descendants pushed earlier), so the host contract
already tolerates this; no suppression needed.

## 8. Milestones

- **M0 — mode plumbing.** `PresenceMode` enum (default `Pop`), builder
  parsing, mode adoption in `update()`, but with pop behaviour not yet
  built: `Pop` temporarily aliases `Wait`. Existing presence tests
  pinned to `mode: Wait` explicitly. Zero behaviour change.
- **M1 — pop machinery.** `Ghost` vec; pop path in `update()`;
  tick/drain; ghost layout pass; `get_children` ordering; exit-
  lifecycle delegation (§5). Tests: immediate mount on key change;
  ghost pinned at frozen rect while live children reflow; ghost drains
  on settle without relayout; multi-cohort rapid switching; revert
  mounts fresh subtree while old ghost keeps dying; Presence-removal
  while ghosts live (begin_exit/is_exit_complete accounting).
- **M2 — hit-test invisibility (§6).** Global `is_exiting()` skip in
  hover, pointer, drag-target, and blocks_point walks. Tests: click
  through a ghost lands on the live button beneath; hover never
  enters a ghost; exiting reconcile ghost (plain flex, no Presence)
  no longer consumes clicks.
- **M3 — prefix semantics (§7).** Pop-time prefix push; drain-time
  suppression for ghosts; test that a same-prefix generation swap
  (same component fn, different key) reports the prefix exactly once,
  at pop time.
- **M4 — docs.** `ANIMATION_LIFECYCLE.md` (Presence section: two
  modes, pop default, interrupt semantics), `LANGUAGE.md` /
  `WIDGET_TREE.md` prop tables, `AGENTS.md` integration note, and the
  changelog line about the default flip (§3).

## 9. Non-goals and follow-ups

- **Perceptual settle** (committing/draining on visual rather than
  numerical completion) — orthogonal `wait`-mode-and-general
  improvement discussed alongside this plan; not part of it.
- **View-level parity.** `ChildStack` `Strict` policy
  (`src/view/mod.rs:610`) deliberately mirrors widget-`Presence`
  wait semantics ("same transition machine, one scale up", Tenet 9).
  Whether the view layer wants a pop-equivalent policy is a separate
  decision with its own doc; nothing here touches it.
- **Focus held by ghosts.** An exiting subtree can hold keyboard focus
  today (wait mode has the same exposure); pop mode does not worsen
  the dispatch path (ghosts are outside `inner.handle_event`), but a
  focused widget popped to ghost should arguably drop focus. Tracked
  as a pre-existing question, not solved here.
- **Ghost z-order option** (paint below live children) — YAGNI until
  a real design asks for it.
