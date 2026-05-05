# Lifecycle + Portal — Untold Lore Validation Audit

> Companion to [`LIFECYCLE_AND_PORTAL.md`](LIFECYCLE_AND_PORTAL.md)
> (the design contract). This document grounds the design in
> Untold Lore's actual UI surface — 20 `.ogh` files plus their
> Rust wrappers across `src/client/` and `src/`. The goal is to
> verify the Phase 2 surfaces (lifecycle hooks, Portal,
> `has_input_blocking_portal`) cover real consumer needs without
> overshooting; flag any missing capability; and produce a
> ranked list of M5 migration candidates.
>
> See [`LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`](LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md)
> for the per-merge plan that consumes this audit.

---

## TL;DR

The Phase 2 design covers UL well. Five concrete findings:

1. **Heavy polling is universal.** Every UL UI re-injects
   most of its host state every tick — 43 `set_host_state`
   call sites total, with the heaviest UIs (Character
   Creation, Settings, Inventory HUD, editors) pushing
   18–25+ values per frame regardless of whether they
   changed. `effect` with deps is the big architectural
   lever; `on_mount` covers static-data seeding for
   moderate-traffic UIs (Character Select, Talents, Tip
   Log).
2. **Two explicit save-on-close handlers** (Settings,
   Ruleset/LifeStages editors); five implicit candidates
   (PlaceInspector, BlueprintEditor, MapEditor, SeaConfig,
   plus Character Creation's "accept" path). Migration is
   straightforward: `on_unmount { event("save_x", form) }`
   replaces the action-handler routing.
3. **Five inline overlay patterns** would become Portal:
   escape menu (the canonical), inventory shop overlay,
   settings keybind-capture (currently no visual popover),
   social create-faction form, character creation review
   panel. Plus three "doesn't exist yet" categories
   (tooltips, true dropdowns, context menus) that Portal
   unlocks.
4. **Zero `Drop` impls in 20 UI wrappers.** Cleanup today
   is implicit — the runtime closes, overlay state clears,
   leaks are silent. `on_unmount` makes cleanup explicit
   and surfaceable per UI.
5. **The push-driven state model composes naturally with
   hooks.** Hook bodies read host state via the existing
   `GetHostState` opcode; nothing in the Phase 2 design
   collides with how UL drives Ogham. Migration is
   incremental — every UI can adopt at its own pace.

**M5 deliverables this audit recommends:**
- Settings: `on_unmount { event("save_settings") }` replaces
  the `CloseSettings` → `actions.rs:2646` round-trip.
- Escape menu: full Portal migration including focus-trap and
  the nested confirm-disconnect dialog.
- One tooltip on the inventory HUD or DM HUD as the worked
  Portal example for non-modal use.
- Optional stretch: Character Creation's review panel as a
  modal-like Portal (high visibility, modest effort).

---

## Cross-cutting findings

### Finding 1 — Per-tick state-push is the universal pattern

Every UL UI wrapper has a `update()` method called from the
client tick loop that re-injects its host state every frame.
Magnitude varies wildly:

| UI | Values pushed per tick | Pattern |
|---|---|---|
| Character Creation | 18+ | Heavy — full `CreationState` snapshot |
| Settings | 17+ | Heavy — full `SettingsFormState` |
| Inventory HUD | 25+ | Heavy — backpack grid + shop |
| Character Select | 10 | Moderate — account + character list |
| Talents | 4 + array | Moderate — level + TP/RP + tree |
| DM Inventory | 7 | Moderate — target + grid |
| Crafting | 3 | Light |
| Console | 3 | Light |
| Tip Log | 2 | Light |
| Chest | 0 | None — content from snapshot directly |
| Map / Blueprint / Ruleset / LifeStages editors | very heavy | Editor state polled comprehensively |
| sea_config | ~10 | Moderate — form fields |
| place_inspector | ~12 | Moderate — place fields + draft |
| dm_hud | varies | Moderate — selection-dependent |
| dm_spawn_picker | 2 arrays | Moderate |
| social | varies | Moderate — faction lists + party state |
| escape_menu | 2 bools | Light — confirm flags |

**Total across UL: 43 `set_host_state(` call sites,
re-pushed every tick.** The runtime's
`set_host_state_if_changed` (added in Phase 1's prelude
work) already deduplicates writes that didn't change value,
so the *runtime cost* is moderate — but the Rust-side cost
of constructing the snapshot (cloning ServerSnapshot fields,
formatting strings, building grids) runs unconditionally.

**Phase 2 lever:**
- `on_mount` for one-shot static seeding (account name,
  campaign metadata, registry snapshots): ~10 of the 43
  calls migrate trivially.
- `effect (server_snapshot.player.health) { ... }` for
  field-watching: most of the moderate-tier UIs collapse to
  a handful of effects.
- The very-heavy editors are the hardest: their state
  changes every frame because the user is interacting (drag,
  paint, draw). `effect` doesn't help when deps change every
  frame anyway. These UIs benefit from `on_mount` for static
  registry data but keep per-tick injection for live editor
  state. Acceptable.

### Finding 2 — Save-on-close is the cleanest `on_unmount` story

UL has two explicit save-on-close paths today:

- **Settings**: `CloseSettings` action at `actions.rs:2646`
  calls `client_settings.save()`. The Ogham UI dispatches
  the close action; the action handler does the I/O.
- **Ruleset / LifeStages editors**:
  `SaveAndExitRulesetEditor` action saves to disk before
  closing. Discard variant closes without saving.

Five implicit candidates:

- **PlaceInspector**: `PlaceInspectorSave` action persists
  changes, but no automatic on-close save — user has to
  click "Save."
- **BlueprintEditor**: `save_blueprint()` called explicitly
  on a button; no on-close save.
- **MapEditor**: same pattern — explicit save button only.
- **SeaConfig**: form changes flow into live scene config
  but disk persistence is opt-in.
- **CharacterCreation**: `accept` dispatches
  `CreateCharacter` action; on cancel, draft state is lost.

**Migration shape:**

```ogh
on_unmount {
  if was_modified {
    event("save_settings", form);
  }
};
```

The Rust handler for `save_settings` does what
`actions.rs:2646` does today; the routing through
`CloseSettings` becomes implicit (Ogham closes the UI;
on_unmount fires; event dispatches; handler saves).

**Important constraint from the design:** state writes from
inside `on_unmount` are discarded (the path has just
unmounted; cleanup pass purges its state cells). The pattern
is `event(...)` dispatch, not state write. The audit
material is consistent with this — every save-on-close
candidate above is naturally an event dispatch, not an
internal state mutation.

### Finding 3 — Five inline overlay patterns would become Portal

Concrete sites where today's inline subtree-swap or
hardcoded-backdrop pattern would benefit from Portal:

| UI | Current pattern | Portal benefit |
|---|---|---|
| Escape menu | Full-screen Flex with `background_color: colors.backdrop`, alpha-160 black; `overlay_state` Rust bool gates input (`update.rs:347, 1469`); confirm-disconnect inline subtree swap | Focus trap; backdrop as ordinary child; `has_input_blocking_portal` collapses overlay_state; nested confirm escapes parent panel bounds |
| Inventory HUD shop overlay | Inline subtree swap based on shop context | Focus trap during transactions; backdrop dismiss-on-outside |
| Settings keybind capture | `rebinding: String` flag mirrored to Ogham; no visual popover, just a highlight | Centered modal showing "Press a key…" with focus trap |
| Social create-faction form | `show_create_faction_form: bool` toggles form visibility inline | Centered modal-style form; backdrop dismiss |
| Character Creation review panel | `review_*` state-driven inline subtree | Modal-style review confirmation before commit |

Plus three "doesn't exist yet" categories:

- **Tooltips**: zero in UL today. Worth adding to inventory
  cells (item details on hover), DM HUD pills (status
  explanation), settings sliders (parameter description).
  All trivial with Portal.
- **True dropdowns**: zero in UL. Settings UI fakes them
  with inline segmented buttons. Future ruleset/blueprint
  editors would benefit from real dropdowns for type
  selection.
- **Context menus**: zero in UL. DM HUD comment at
  `dm_hud.ogh:16–20` explicitly notes this absence. Right-
  click menus on map entities, inventory items, NPC pills
  all become trivial.

### Finding 4 — Zero `Drop` impls means cleanup is silent

Across 20 UI wrapper structs in UL, **none implement
`Drop`**. Cleanup happens implicitly: the Ogham runtime
unmounts, `overlay_state` clears, in-flight server requests
complete or are abandoned, no explicit teardown runs.

This isn't broken — UL is mostly request-response with
server-authoritative state, so resource leaks are bounded
(per-UI Arc<Mutex> structures fall out of scope when the
wrapper drops, file handles aren't held). But it does mean:

- Editor undo/redo stacks are dropped silently.
- In-flight network requests aren't proactively cancelled
  (the response handler is just no-op'd).
- Cached snapshot state in form structures is discarded
  without flushing.

`on_unmount` makes cleanup *visible* in the `.ogh`. The
audit recommends a per-UI cleanup pass during M5 — not
because anything's broken, but because the explicit
declaration surfaces invariants that today live in the
"nothing happens because it doesn't have to" gap.

### Finding 5 — UL has no second-class lifecycle the design needs to integrate with

A risk going in: UL might have its own lifecycle concepts
(scene transitions, server reconnects, character switch)
that needed to integrate with Phase 2 hooks in a specific
way. **It doesn't.**

UL's higher-level state transitions (login → character
select → in-game → escape menu) are managed at the
client/mod.rs level by swapping which Ogham instance is
active. From the Ogham runtime's perspective, this is just
"a different module loaded" — the previous module's state
gets cleaned up by the standard runtime teardown when its
runtime is dropped.

This means Phase 2 hooks can ignore UL-level scene
transitions — they fire at module-instance lifecycle, which
is what UL already drives. No special integration work
needed.

---

## Design implications distilled

What UL teaches us about the Phase 2 design.

### The Portal API surface is correctly minimal

The five concrete Portal candidates above all decompose
into `(open, focus_trap, children)`. None need anchor
positioning as a Portal property (transform handles it),
backdrop as a property (first child handles it), or
escape-to-dismiss as a property (consumer key handler
handles it). The minimal-API decision (decisions #9, #10,
#11 in the design's resolved-decisions table) holds up
against real consumer usage.

### `effect` is more important than initially weighted

The design weights all three hook kinds equally. This audit
suggests `effect` is the highest-leverage piece for UL
because the per-tick polling pattern is universal. A typical
UL UI today:

```rust
// Rust update() called every tick
fn update(&mut self) {
    let snapshot = server_snapshot();
    self.ogham.set_host_state("player_health", snapshot.player.health);
    self.ogham.set_host_state("player_mana", snapshot.player.mana);
    // ... 17 more
}
```

After Phase 2, the UI declares which fields it cares about:

```ogh
effect (host_state.player_health) { /* react */ };
effect (host_state.player_mana)   { /* react */ };
```

The Rust side still pushes the snapshot (Ogham doesn't pull
from Rust), but reactive logic moves into `.ogh` where it
belongs. This is a meaningful shift in where logic lives,
not just a syntactic cleanup.

### State writes from `on_unmount` need a sharp doc warning

Every save-on-close candidate in UL is naturally an
`event(...)` dispatch (Rust handler does the I/O). But the
"intuitive" temptation for someone migrating would be:

```ogh
on_unmount {
  // WRONG — writes are discarded
  state.committed_form = compute_committed_form(form);
};
```

The design's body-scope section flags this. The audit
recommends elevating it to a numbered design decision and
adding a hover-tip in the LSP for `on_unmount` keyword
("state writes here are discarded; dispatch an `event(...)`
for persistence").

### Conditional hook risk is real (Settings example)

Settings has tab-conditional setup — the keybinds tab
registers different event handlers than the audio tab.
Today this is all done in Rust at construction. In a
naive `on_mount` migration:

```ogh
let settings = fn (active_tab) {
  if active_tab == "keybinds" {
    on_mount { register_keybind_handlers(); };  // foot-gun!
  }
};
```

Per the design (decision #16), this is legal but the LSP
warns. The warning text needs to be specific enough that a
UL author migrating Settings recognizes the pattern and
restructures to:

```ogh
on_mount { register_keybind_handlers(); };

effect (active_tab) {
  if active_tab == "keybinds" {
    enable_keybind_capture();
  } else {
    disable_keybind_capture();
  };
};
```

The audit confirms the warning is doing real work, not
hypothetical work.

### `has_input_blocking_portal` as the ONE runtime API is correct

UL needs exactly one signal from Ogham about portal state:
"should I gate world input?" The audit found no other
runtime-side question UL would ask about portal state. No
need for `active_portals()`, no need for `portal_count()`,
no need for per-portal queries. The single boolean handles
the case.

### Hook composition isn't blocking

The design defers "hook composition / useX helpers." The
audit confirms this is fine for UL: the recurring patterns
that *would* benefit from helper hooks (form-state
mirroring, server-snapshot polling, save-on-close) are
each used 1–3 times. Inlining them in each UI is acceptable;
factoring into shared `fn`s in a `components.ogh`-style
file handles the few places repetition would matter.

---

## UL migration impact

Quantitative estimates of the post-Phase-2 picture.

### Lines of Rust UI code likely to migrate to `.ogh`

| Category | Today | After Phase 2 | Delta |
|---|---|---|---|
| `set_host_state` call sites | 43 | ~22 | −21 (−49%) |
| `update()` per-tick complexity (avg LOC per UI) | ~25 | ~10 | −15 (−60%) |
| Explicit `Close*` action variants | 14 | ~10 | −4 (−29%) |
| Per-UI Arc<Mutex<FormState>> mirrors | 8 | ~5 | −3 (−38%) |
| `overlay_state: Option<...>` plumbing in `update.rs` | ~30 LOC | ~5 LOC | −25 |
| `confirm_*: bool` flags in `mod.rs` | ~6 | 0 (move into `.ogh` `state`) | −6 |

Estimates are rough — they assume a thorough migration of
heavy UIs. Light UIs (Tip Log, Chest, Crafting) gain less
because they have less to migrate.

### Lines of `.ogh` likely to grow

| Category | Today | After Phase 2 | Delta |
|---|---|---|---|
| `on_mount` blocks across all UIs | 0 | ~20 | +20 |
| `on_unmount` blocks | 0 | ~12 | +12 |
| `effect` blocks | 0 | ~30 | +30 |
| Portal usages | 0 | ~8 | +8 |
| Tooltip / dropdown / context-menu fns in `components.ogh` | 0 | ~3 | +3 |

Total `.ogh` LOC growth: ~150–250 lines across UL,
heavily concentrated in the migrated UIs. Net codebase
size change is negative (Rust losses outweigh Ogham gains).

### New capabilities unlocked

- **Tooltips** on inventory cells, DM pills, settings sliders.
- **True dropdowns** in Settings, ruleset editor, blueprint
  type pickers.
- **Context menus** on map entities (right-click → "Inspect /
  Possess / Spawn here").
- **Modal stacking** in escape menu and confirm dialogs
  without parent-bound layout constraints.
- **Focus isolation** during sensitive operations (settings
  keybind capture, shop transactions).

### Per-frame work reduction

Hand-wavy but directional: in the heavy UIs (Character
Creation, Settings, Inventory HUD, editors), per-tick
allocation drops by ~70% if `effect` adoption is thorough.
The `set_host_state_if_changed` deduplicator already saves
the no-op writes; the bigger win is not constructing the
values in the first place.

For light UIs, the reduction is negligible (they don't push
much anyway). For editor-class UIs, the reduction is
modest because editor state genuinely changes every frame
during interaction.

---

## Suggested migration sequence

### M5 ships (validation gate)

| # | Migration | Why M5 |
|---|---|---|
| 1 | Settings save-on-close (`CloseSettings` → `on_unmount` + event) | Smallest surface; exercises `on_unmount` + event dispatch in isolation |
| 2 | Escape menu → full Portal (focus_trap, backdrop child, nested confirm) | Highest leverage; exercises every Portal feature |
| 3 | One tooltip on inventory HUD (worked example) | Validates non-modal Portal pattern; smallest possible new-capability proof |

These three are the canonical M5 deliverable. Together they
exercise: `on_unmount` + `event`, Portal with `focus_trap`,
nested portals, backdrop-as-child, dismiss-on-outside,
`has_input_blocking_portal`, transform-positioned non-modal
Portal.

### Post-Phase-2 backlog (in suggested order)

| # | Migration | Estimated effort | Value |
|---|---|---|---|
| 4 | Character Select: `on_mount` for static account/campaign data | 0.5 day | Eliminates 10 set_host_state per tick |
| 5 | Talents UI: `effect (server.talents)` + `on_mount` | 0.5 day | Cleaner; 4 fewer pushes per tick |
| 6 | Tip Log: `on_mount` for static config; effect for new tips | 0.5 day | Marginal but easy |
| 7 | Inventory HUD shop overlay → Portal (focus_trap, backdrop) | 1 day | Real UX improvement |
| 8 | Settings: `effect (active_tab) { ... }` for tab-specific setup | 1 day | Validates the conditional-hook warning |
| 9 | Settings: keybind-capture as Portal | 0.5 day | Fixes existing UX issue |
| 10 | DM HUD: tooltips on status pills | 0.5 day | New capability |
| 11 | Character Creation: review panel as Portal | 1 day | Visual polish |
| 12 | Social: create-faction as Portal | 0.5 day | Modal feel |
| 13 | DM context menu (right-click on entities) | 1 day | Brand-new capability per dm_hud.ogh:16–20 |
| 14 | Editor UIs (Map, Blueprint, Ruleset, LifeStages): `on_mount` for registry data; keep per-tick for editor state | 2 days each | Modest per-UI; high cumulative |
| 15 | `place_inspector` save-on-unmount | 0.5 day | Quality-of-life |
| 16 | `sea_config` save-on-unmount | 0.5 day | Quality-of-life |

Total post-Phase-2 backlog: ~15 person-days. Can be done
incrementally as time allows; nothing blocks.

### What we explicitly don't migrate in Phase 2

- **Editor UIs' core state.** Map / Blueprint / Ruleset /
  LifeStages editors keep their Rust-side editor state
  (selection, undo/redo, drag state) in Rust. Only the
  static config (registry snapshots) moves to `on_mount`.
  Reason: editor state changes every frame during
  interaction; effect would re-fire constantly.
- **Server-snapshot mirroring.** Most UIs would still
  receive ServerSnapshot via `set_host_state` from Rust,
  rather than reading via some pull mechanism. Phase 2
  doesn't add a pull side; that's a separate concern (and
  arguably correct as-is — Rust knows when the snapshot
  changed, Ogham doesn't).
- **Cross-UI state coordination.** UL's pattern of swapping
  which Ogham instance is active stays the same. Phase 2
  hooks operate within a single instance.

---

## Open questions raised by the audit

Things the audit surfaced that the design doc should address
or explicitly punt on.

1. **What happens to `on_unmount` when the host crashes?**
   On graceful close, `on_unmount` fires and the
   `save_settings` event dispatches normally. On crash, the
   Rust process dies before the runtime's drain pass runs.
   UL's existing pattern (Settings calls `.save()` from the
   action handler, which runs synchronously before the UI
   actually closes) is robust to this; moving to
   `on_unmount` introduces a small window where settings
   changes could be lost if the user crashes between "made
   change" and "closed UI." Worth documenting; the design
   should spell out that `on_unmount` is best-effort, not a
   transactional commit.

2. **Effect deps on host_state — re-evaluation cost.**
   `effect (host_state.player_health) { ... }` evaluates
   `host_state.player_health` every render to compare. For
   a UL frame with ~60 effects across all UIs, that's ~60
   `GetHostState` opcodes + value comparisons per render.
   Probably fine; revisit if profiling shows it.

3. **Portal `open` toggling mid-exit-animation.** The design
   §13 says exit animations on children run when `open:
   false`. But what if `open` flips back to `true` while
   exit is in flight? Per ghost-cancellation semantics in
   ANIMATION_LIFECYCLE.md, the exit cancels and the children
   re-mount. Confirm this works correctly through the
   Portal layer in M3 testing — adding to the impl plan's
   open-questions list.

4. **`focus_trap` on escape menu's nested confirm-disconnect
   portal.** The escape menu has `focus_trap: true`; the
   nested confirm-disconnect inside it is also a Portal
   with `focus_trap: true`. The design says
   "last-opened-wins focus-trap" (decision #12). Validate
   that the nested case does what the user expects:
   focus trapped within confirm-disconnect, escape from
   confirm-disconnect returns focus to escape menu (not
   the world).

5. **Tab-conditional hook patterns.** Settings will exercise
   the conditional-hook LSP warning in M5. Decide before
   M5: does the warning have a `// allow_conditional_hook`
   suppression syntax for cases where the author explicitly
   wants the path-mount semantics, or is the warning
   purely advisory? Recommendation: advisory only for now.

6. **What about hot-reload during a focus-trapped portal?**
   If the escape menu is open with focus trapped, and the
   author hot-reloads the escape menu's `.ogh` file, what
   happens to the focus stack? Per design open question #6,
   focus stack clears on hot-reload. UL's escape menu being
   the validation gate means this gets tested in M5
   naturally.

7. **State migration for editor UIs.** When Map / Blueprint
   editors eventually adopt hooks (post-Phase-2), what
   happens to their existing Arc<Mutex<EditorState>>?
   Likely: stays in Rust (editor state is too stateful to
   move into `state` cells), but `on_mount` seeds initial
   values and `on_unmount` flushes on close. Audit
   recommends keeping the audit's per-UI section for
   editors deliberately light — they're the smallest
   migration leverage relative to their complexity.

8. **`Portal` as a built-in widget — registration order.**
   Per design, `Portal` registers in
   `WidgetRegistry::with_defaults()`. UL doesn't override
   defaults (it adds custom widgets via `with_widget`).
   Confirm at M3 that the default registration is visible
   to UL without code changes.

9. **Dropdown `fn` in `examples/portals/components.ogh`.**
   The design ships `Modal()` and `Tooltip()` as library
   `fn`s. The audit suggests adding `Dropdown()` too —
   Settings is a clear consumer that would adopt it
   immediately. Should the Phase 2 examples library include
   `Dropdown()`, or punt to a follow-up? Recommendation:
   include in M5 since the implementation cost is small
   (~30 LOC of `.ogh`) and the consumer demand is concrete.

10. **`focus_trap` with non-focusable subtree.** What happens
    if a `focus_trap: true` Portal's children contain no
    focusable widgets? Focus has nowhere to go; the trap
    succeeds at "preventing focus from leaving" but does
    nothing else. Probably fine, but worth a unit test.

---

## Per-UI detail

20 UIs in dependency-loose order. Each section names: what
the UI does (one sentence); the setup pattern with line
citations; the teardown pattern with line citations; overlay
patterns; per-tick polling pattern; and a migration verdict
(High / Medium / Low value, with one-line justification).

### character_select

**Does:** Roster of playable characters per account, with
campaign / map / adventure context and "play / disconnect"
actions.

**Setup:** Initial host_state seed of 10 values
(`account_display_name`, `characters`, `confirm_disconnect`,
`server_error`, `maps`, `adventures`, `campaign_name`,
`selected_character_id`, `is_pending`, `can_run_as_dm`) at
`character_select_ui.rs:67–86`. Event handlers for row
clicks at `:105–168`.

**Teardown:** No save-on-close; selection state lives in
`Arc<Mutex>`. `CloseCharacterSelect` action exists but
doesn't trigger flush.

**Overlays:** None.

**Per-tick polling:** `update()` at `:204` re-injects all
10 values every frame at `:277–286`.

**Migration verdict — Medium.** `on_mount` migrates 6 of
the 10 values (the static `account_display_name`,
`campaign_name`, `can_run_as_dm`, plus the `maps` and
`adventures` lists which only change on hub state events).
The remaining 4 (selection-related) need `effect` on
selection state. Not high priority; UI works fine today.

### character_creation

**Does:** Backstory decision-tree walker with stage-by-
stage navigation, optional trait toggles, and final
character generation.

**Setup:** RegistrySnapshot captured at construction
(`character_creation_ui.rs:147–150`); trait defs filtered at
`:154–163`; `Arc<Mutex<CreationState>>` seeded at
`:177–184`; 8 event handlers at `:206–311`.

**Teardown:** `accept` dispatches `CreateCharacter` action
(server-handled). On cancel, `CancelCharacterCreation`
discards state — no save.

**Overlays:** Inline review panel before commit (state-
driven subtree swap based on `review_*` host_state fields).
Could become a Portal — validates "modal review" pattern.

**Per-tick polling:** `tick()` at `:330` re-seeds 18 values
each frame at `:493–519` (`character_name`, `target_mode`,
`committed_rows`, `traits_row_*`, `active_stage_*`,
`pending_traits_*`, `review_*`, `can_accept`,
`server_error`).

**Migration verdict — High.** Heavy polling with mostly
slow-changing values. `on_mount` for registry-derived
defaults (~6 values); `effect (active_stage_id)` for
stage-derived values (~8 values); `effect (pending_traits)`
for derived target_mode and can_accept. Plus the review
panel Portal migration is a worked example for non-escape-
menu modal usage.

### settings

**Does:** In-game settings overlay with audio, video,
controls, gameplay, keybind tabs.

**Setup:** SettingsFormState created from ClientSettings at
`settings_ui.rs:90`; 23+ event handlers at `:98–275`;
initial state pushed at `:92`.

**Teardown:** `close_settings` handler at `:269–274`
dispatches `CloseSettings` action; `actions.rs:2646` calls
`client_settings.save()`. **Canonical save-on-close
candidate.**

**Overlays:** Inline tab system (`active_tab` swaps panes).
Keybind capture currently shows no popover — just a
highlight on the rebinding row. Strong Portal candidate
for a centered "Press a key…" modal during capture.

**Per-tick polling:** `update()` at `:283–291` re-injects
~17 values each tick. `refresh_from()` at `:307–310` and
`set_rebinding()` at `:314–317` are called from
`actions.rs` when external state changes.

**Migration verdict — High.** Two distinct migrations:
1. M5: `on_unmount { event("save_settings", form) }`
   replaces the action-handler routing.
2. Post-M5: keybind capture as Portal with focus_trap;
   tab-conditional `effect` for tab-specific setup
   (tests the conditional-hook warning).

### escape_menu

**Does:** Paused-game overlay with Resume / Settings /
Social / Tips / Disconnect / Reset Run, plus nested confirm
dialogs for Disconnect and Reset.

**Setup:** Built inline at `client/mod.rs:2163` via
`Self::build_escape_ogham()`. No separate UI wrapper.
Confirmation state stored as `confirm_disconnect`,
`confirm_reset_run` bools at `mod.rs:1209–1210`.

**Teardown:** `CloseEscapeMenu` at `actions.rs:2379` clears
`overlay_state` to `None`. View-only; no save.

**Overlays:** Full-screen Flex with
`background_color: colors.backdrop` (alpha-160 black) at
`escape_menu.ogh:100–211`. Input gating via `overlay_state`
in `update.rs:347, 1469` — *not* an Ogham concept. Two
nested confirm sub-panels swap inline via `match`.

**Per-tick polling:** None — confirmation state pushed via
`set_host_state` in actions.rs on user clicks. No polling
of derived state.

**Migration verdict — Highest leverage.** The canonical
M5 migration. Becomes a Portal with `focus_trap: true`;
backdrop is the first child; nested confirm-disconnect is
its own nested Portal; `overlay_state: Option<...>`
collapses to `let overlay_active =
ogham.has_input_blocking_portal()` on the Rust side.
`confirm_*` bools move from `mod.rs` into `state` cells in
`escape_menu.ogh`.

### inventory_hud

**Does:** Player inventory grid + equipped tool + optional
shop overlay.

**Setup:** Initial state at `inventory_hud.rs:43`; 7 event
handlers at `:47–126`.

**Teardown:** `CloseOverlayPanel` action on shop close. No
explicit inventory save (server-driven).

**Overlays:** Shop overlay is inline subtree swap based on
`show_inventory` and `shop` context. Strong Portal
candidate for the shop (focus_trap during transactions).
Tooltip candidate on inventory cells (item details on
hover).

**Per-tick polling:** Heavy — `update()` at `:143`
re-injects ~25 values from ServerSnapshot (backpack grid
cells, shop inventory, currency, skills, tools).

**Migration verdict — High.** Three distinct migrations:
1. M5: tooltip on inventory cells (the worked non-modal
   Portal example).
2. Post-M5: shop overlay → Portal with focus_trap.
3. Post-M5: `effect` per ServerSnapshot field (collapses
   per-tick allocation by ~80%).

### dm_hud

**Does:** DM-mode HUD showing pause status pill +
property inspector for current selection.

**Setup:** Initial state at `dm_hud_ui.rs:27`; 6 event
handlers at `:30–71`.

**Teardown:** None (always-on in DM mode).

**Overlays:** None today, but tooltip candidates on the
status pills (explain mode meanings) and context-menu
candidates on the inspector entries.

**Per-tick polling:** Moderate — `update()` at `:78`
re-injects from DmMode and ServerSnapshot.

**Migration verdict — Medium.** Tooltips on status pills
are a quick win (M5 stretch). Context menus would unlock
the "right-click NPC → Inspect / Possess / Spawn here"
pattern explicitly noted as missing at `dm_hud.ogh:16–20`
— but that's a feature add, not a migration, and probably
post-Phase-2.

### dm_inventory

**Does:** Foreign-inventory editor (DM takes/grants items).

**Setup:** Initial state at `dm_inventory_ui.rs:33`; 4
event handlers at `:36–73`. Target CharacterId locked at
`:79`.

**Teardown:** `DmCloseInventory` action on close. No save
(server validates).

**Overlays:** None (inline within DM HUD layout).

**Per-tick polling:** Moderate — `update()` at `:96`
re-injects 7 values at `:204–212` (target_name, target_kind,
inv dimensions, rows grid, grant_buffer).

**Migration verdict — Low.** Could `on_mount` the static
target details, `effect` the grid changes. Not a high
priority — UI works fine.

### dm_spawn_picker

**Does:** Two-column NPC × spawn-point picker for combat
spawns.

**Setup:** Initial state at `dm_spawn_picker_ui.rs:30`; 2
event handlers at `:33–53`.

**Teardown:** `DmCloseSpawnPicker` on close. No save.

**Overlays:** None.

**Per-tick polling:** Modest — `update()` at `:60`
re-injects npc_kinds and spawn_points at `:102–118`.

**Migration verdict — Low.** Move kinds and spawn_points
to `on_mount` (they only change on world load). Trivial.

### chest

**Does:** Minimal pickup UI for static containers.

**Setup:** 2 event handlers at `chest_ui.rs:17–30`. No
state seeding.

**Teardown:** `ChestCancel` on close. No save.

**Overlays:** None.

**Per-tick polling:** None — `update()` at `:38` is empty.

**Migration verdict — None.** UI is data-light; nothing to
migrate. Mentioned in the Phase 1 typed-bindings audit as
a typed-bindings test consumer; Phase 2 leaves it alone.

### crafting

**Does:** Crafting station UI showing recipes and triggering
crafts.

**Setup:** Initial state at `crafting_ui.rs:34`; 2 event
handlers at `:41–56`; `pending_until` debounce at `:70`.

**Teardown:** `CloseCrafting` on close. No save.

**Overlays:** None.

**Per-tick polling:** Moderate — `update()` at `:74`
re-injects 3 values at `:109–111` (station_name, recipes,
is_pending).

**Migration verdict — Low.** `on_mount` for station_name
and recipes (server-load-triggered). `effect` for
is_pending. Quick win but low value.

### console

**Does:** In-game developer console with command parsing
and history.

**Setup:** Minimal — history, history_count, input_value at
`console_ui.rs:29–33`; 2 event handlers at `:37–48`.

**Teardown:** `CloseConsole` on close. No save (history is
ephemeral).

**Overlays:** None.

**Per-tick polling:** Light — `update()` at `:79`
re-injects history_count, history array, input_value at
`:97–99`.

**Migration verdict — Low.** `effect (console.history)`
for the history. Not pressing.

### tip_log

**Does:** Toast/notification log display.

**Setup:** Minimal at `tip_log_ui.rs:19–22`; 1 event
handler at `:26–31`.

**Teardown:** `CloseTipLog` on close. No save.

**Overlays:** None.

**Per-tick polling:** Light — `update()` at `:40`
re-injects tip_count and tips array at `:51–52`.

**Migration verdict — Low.** Trivial `effect` candidate.

### social

**Does:** Faction and party management with creation
forms, request acceptance, member browsing.

**Setup:** FormState built at `social_ui.rs:34–40`; ~15
event handlers at `:48–250+`.

**Teardown:** `CloseSocial` on close. No save (server-
driven).

**Overlays:** Inline form toggle for create-faction
(`show_create_faction_form` bool). Portal candidate.

**Per-tick polling:** Moderate — re-injects faction lists,
party lists, request counters.

**Migration verdict — Medium.** Create-faction form as
Portal (modal feel); `effect` for the list re-injections.

### talents

**Does:** Talent tree display with learn/respec actions.

**Setup:** Initial state at `talents_ui.rs:24–29`; 2 event
handlers at `:33–60`.

**Teardown:** `CloseTalents` on close. No save.

**Overlays:** None today; tooltip candidates on individual
talents (description, requirements).

**Per-tick polling:** Moderate — `update()` at `:68`
re-injects 4 values at `:167–170`.

**Migration verdict — Medium.** `on_mount` for char_level
(infrequent change); `effect` for talent_points and tree.
Tooltips on talents would be a UX win.

### life_stages_editor

**Does:** Authored backstory decision-tree editor.

**Setup:** Complex (44K-line wrapper). RegistrySnapshot;
many initial fields.

**Teardown:** Exit handler saves the working copy back to
disk. Save-on-close candidate.

**Overlays:** Inline node editor; no modals.

**Per-tick polling:** Heavy.

**Migration verdict — Medium-deferred.** `on_mount` for
registry data (~5 values). `on_unmount` for save-on-close.
Per-tick editor state stays. Defer to post-M5.

### blueprint_editor

**Does:** Building blueprint editor (walls, openings,
materials).

**Setup:** Complex (660-LOC wrapper). Editor state
initialized.

**Teardown:** Explicit `save_blueprint()`; no on-close save
today.

**Overlays:** None (full-screen editor).

**Per-tick polling:** Heavy editor state.

**Migration verdict — Medium-deferred.** Add on_unmount
for explicit save-on-close (currently relies on user
button). Editor state stays in Rust.

### map_editor

**Does:** Full world map editor (terrain, structures, POIs,
NPCs, spawn points, biome painter).

**Setup:** Complex (3.8K-line wrapper). WorldMapEditor +
Ogham UI wrap.

**Teardown:** Explicit save / discard via toolbar; no
on-close.

**Overlays:** Multiple sidebar panels; inline subtree swaps
based on active tool.

**Per-tick polling:** Very heavy.

**Migration verdict — Medium-deferred.** Same shape as
blueprint editor. on_unmount for save-on-close;
on_mount for catalog data; per-tick editor state stays.

### ruleset_editor

**Does:** Ruleset authoring (progression, NPC kinds,
recipes, shops, loot).

**Setup:** Complex (23K-line wrapper). Ruleset snapshot
loaded; initial state seeded.

**Teardown:** `SaveAndExitRulesetEditor` saves to disk.
`DiscardRulesetEditor` closes without save. Save-on-close
canonical pattern.

**Overlays:** Inline table/detail panels. No modals.

**Per-tick polling:** Heavy.

**Migration verdict — Medium-deferred.** on_mount for the
table-list data; on_unmount for the save (matching
Settings's pattern); per-tick editor state stays.

### sea_config

**Does:** Water rendering parameters editor (sea level,
turbulence, waves, colors, foam).

**Setup:** SeaConfigFormState at `sea_config_ui.rs:78`;
~20 event handlers at `:86–200+`; initial state pushed at
`:80`.

**Teardown:** Discard closes without persisting. Confirm
saves. Save-on-close candidate.

**Overlays:** None.

**Per-tick polling:** Moderate.

**Migration verdict — Medium.** on_unmount for
save-or-discard. `effect` per form field.

### place_inspector

**Does:** In-game place editor (name, description, kind,
discovery radius, attachments, arrival mode).

**Setup:** Initial state at `place_inspector_ui.rs:24–52`;
~8 event handlers at `:56–162`.

**Teardown:** `PlaceInspectorSave` persists;
`PlaceInspectorClose` discards. Save-on-close candidate.

**Overlays:** None.

**Per-tick polling:** Moderate — `update()` at `:172`
re-injects place fields and draft state.

**Migration verdict — Medium.** on_unmount for save-or-
discard; effect per place-snapshot field.

---

## Quantitative summary

### Per-UI migration matrix

| UI | on_mount | on_unmount | effect | Portal | Verdict |
|---|---|---|---|---|---|
| character_select | ✓ (6) | — | ✓ (4) | — | Medium |
| character_creation | ✓ (6) | — | ✓ (8) | ✓ (review) | High |
| settings | — | ✓ (save) | ✓ (tab) | ✓ (keybind) | High |
| escape_menu | — | — | — | ✓ (canonical) | Highest |
| inventory_hud | — | — | ✓ | ✓ (shop, tooltip) | High |
| dm_hud | — | — | ✓ | ✓ (tooltip, ctx menu) | Medium |
| dm_inventory | ✓ | — | ✓ | — | Low |
| dm_spawn_picker | ✓ | — | — | — | Low |
| chest | — | — | — | — | None |
| crafting | ✓ | — | ✓ | — | Low |
| console | — | — | ✓ | — | Low |
| tip_log | — | — | ✓ | — | Low |
| social | — | — | ✓ | ✓ (form) | Medium |
| talents | ✓ | — | ✓ | ✓ (tooltip) | Medium |
| life_stages_editor | ✓ | ✓ (save) | partial | — | Medium-deferred |
| blueprint_editor | — | ✓ (save) | — | — | Medium-deferred |
| map_editor | ✓ | ✓ (save) | partial | — | Medium-deferred |
| ruleset_editor | ✓ | ✓ (save) | — | — | Medium-deferred |
| sea_config | — | ✓ (save) | ✓ | — | Medium |
| place_inspector | — | ✓ (save) | ✓ | — | Medium |
| **Totals** | **~10** | **~7** | **~14** | **~8** | |

### Bottom line

- **High-leverage migrations: 4** (Character Creation,
  Settings, Inventory HUD, Escape Menu).
- **Medium migrations: 6** (Character Select, DM HUD,
  Social, Talents, Sea Config, Place Inspector).
- **Deferred-or-low: 6** editors and minor UIs.
- **No migration: 1** (Chest, used as the typed-bindings
  test consumer).

**M5 ships 3 migrations** (Settings save, Escape menu
Portal, one tooltip) covering the canonical patterns.
**Post-M5 backlog: ~12 person-days** spread across the
remaining migrations, taken at the team's pace.

---

## Next steps

1. The Phase 2 design doc is unchanged by this audit — all
   the design decisions hold up under real consumer usage.
   Specifically, the minimal Portal API survives, the
   conditional-hook LSP warning earns its keep (Settings
   tab-conditional setup is a real example), and
   `has_input_blocking_portal` is the right runtime API
   shape.
2. Two design-doc additions recommended (small):
   - Add a sharper warning to the body-scope section about
     state writes from `on_unmount` being discarded
     (UL audit shows this is the most common temptation).
   - Add `Dropdown()` to the `examples/portals/components.ogh`
     reference library shipped in M5 (Settings is a clear
     immediate consumer).
3. The implementation plan
   ([`LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`](LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md))
   should treat the M5 deliverables as the ones above:
   Settings save-on-close, escape menu full Portal, one
   tooltip. Plus the `Dropdown()` example fn.
4. Post-Phase-2 backlog goes into project tracking, not
   into the live contract docs. The audit's per-UI
   migration verdicts are the source of truth for that
   backlog.
5. Open questions raised by this audit (especially #1
   about crash-safety of `on_unmount` and #6 about hot-
   reload during focus-trapped portals) should be folded
   into the implementation plan's open-questions list.

This audit graduates `LIFECYCLE_AND_PORTAL.md` from
"Live design contract — implementation pending" to "Live
design contract — UL-validated, implementation pending."
The remaining unknown is execution risk, not design risk.
