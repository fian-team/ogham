# Ogham — The Application Build

> **Status: implementation plan for `APPLICATION.md`.** Written
> 2026-08-20 from the four-repo consumer audit. Audience: an
> orchestrating agent driving subagents through the entire suite of
> changes. `APPLICATION.md` is the design record — this document
> never restates its axioms, only cites them (§n.n references are
> into APPLICATION.md unless prefixed with a repo doc name).

---

## 0. How to use this document

**Read `APPLICATION.md` first, in full.** Every work package below
implements some axiom of it; a subagent that has not read the
axioms will reinvent the holes they patch.

**Repos** (siblings of `~/Projects/ogham`):

| repo | role |
|---|---|
| `ogham` | surface framework + (today) the route tier |
| `lorekeeper` | engine: `app`, `front`, `shell`, `shell-core`, `services`, `editable*`, `ogham_preview` |
| `untold_lore` | consumer: `ul-client`, `ul-editor` |
| `regency` | consumer: `regency-client`, `regency-core`, `regency-sheet` |
| `stargazer-celia-game` | consumer: `celia-client`, `celia-core` |

**Workflow rules** (standing, from the maintainer):

- All work lands on `main` directly — no feature branches. Every
  work package must leave its repo green (`cargo test`) when it
  lands. Order the packages so this is possible; where a package
  cannot land green alone, its section says what it pairs with.
- Ogham CI is red at baseline (clippy/fmt debt). Measure clippy and
  fmt deltas against a stashed baseline, not against zero.
- Commit messages are single declarative sentences in the repo's
  existing voice (read `git log --oneline -20` before writing one).

**Line-number caveat.** Every `file:line` below is a 2026-08-20
anchor from the audit. Re-locate by **symbol name**, not line
number; if a named symbol is gone, stop and re-audit that package
before proceeding — do not guess.

**Subagent protocol.** One work package per subagent. Give each
subagent: the WP text below, the APPLICATION.md sections it cites,
and the acceptance criteria as its exit contract. Packages marked
∥ may run concurrently (different repos or disjoint crates);
everything else is sequential. After each phase, run the phase
gate before starting the next. When a WP says **DECISION**, stop
and put the question to the maintainer — do not resolve it by
fiat.

**Naming placeholder.** Crate names below (`structure`, `driver`)
are working names. APPLICATION.md §8.4 defers real naming until
the seam is settled; do not bikeshed names mid-build, and do not
publish any crate.

## 1. Phase graph

```
P0 (ogham bug fixes)                          — lands alone, now
 └─ P1 (structure crate extraction)
     └─ P2 (the store)
         ├─ P3 (surface framework: modules, selections)   ∥ with P4
         └─ P4 (driver: binding + crossing)               ∥ with P3
             └─ P5 (front: schemas + foh root)
                 ├─ P6C (celia migration)      ∥
                 ├─ P6R (regency migration)    ∥
                 └─ P6U (untold_lore migration) — then P6S (shell deletion)
                     └─ P7 (docs)
```

P3 and P4 are parallel (different repos). The three game
migrations are parallel with each other. P6S (deleting
`lorekeeper/shell`) waits on P6U only, because `ul-client` and
`ul-editor` are the last consumers of `shell`.

---

## Phase 0 — pre-plan bug fixes (repo: ogham + lorekeeper)

These are bugs under the *current* design and land independently
of everything else. Do them first so hot-reload behavior is sound
before the store inherits it.

### WP-0.1 Hot reload revalidates and re-projects

Today validation runs once, at `RouterHost::new`
(`lorekeeper/front/src/host.rs:91-119` →
`ogham/src/route/chrome.rs:138-161`), and hot reload happens
silently inside `Ogham::frame` (`ogham/src/lib.rs:547-556`).
Nothing re-runs validation after a reload, and
`Chrome::forget_projection` (`chrome.rs:236-238`) — written
because "a hot reload replaces the runtime, so every key has to be
pushed again" — has **zero callers** in five repos. Consequences:
a drift introduced by a hot edit is silent until restart, and
`Chrome::last` (`chrome.rs:29`) survives the runtime swap, so
unchanged keys leave the fresh document holding config defaults.

Change: surface "a reload happened" from `Ogham::frame`'s return;
on reload, `Chrome` calls `forget_projection()` and re-runs
`validate`/`validate_raises`, reporting by the existing path.

Acceptance: a test in ogham proving that after a simulated reload
(a) every key re-projects, (b) an id/raise drift introduced by the
edit produces a named report without restart. All existing tests
green in ogham and lorekeeper.

---

## Phase 1 — the structure crate (repo: ogham)

### WP-1.1 Extract routing from the surface framework

Reverse the placement half of ROUTING.md phase 8c:
`ogham/src/route/{table,router,outbox}` moves to a new workspace
crate (working name `structure`). `chrome.rs` splits three ways —
its per-key diff/inject core (`chrome.rs:179-211`) is the seed of
the store (stays behind, absorbed in P2); its validation
(`chrome.rs:90-161`) becomes the selection checker (P2); its
`Ogham`-touching remainder moves up into the driver (P4). Until P4
lands, `front::RouterHost` keeps working against a thin re-export.

The engineering guarantee is §2's dependency edge. Acceptance:
`cargo tree -p structure` shows no dependency on ogham;
`cargo tree -p ogham` shows no dependency on structure; all
consumers compile via the re-export; tests green.

### WP-1.2 The table grows tiers, areas, guards, documents

Implements §3.2 and §3.4 as table data:

- Node tier: `instance_root(document_path)` | `view` |
  `structural` (owns a scope, mounts/selects nothing).
- `area` attribute on instance roots (replaces untold_lore's
  `crossing::side_of`, `ul-client/src/crossing.rs:51-57` — four
  roots, three areas; preserve the catch-all-default property
  noted at `crossing.rs:162-165`).
- Occlusion becomes static node data, not a method
  (`router.rs:250-272` `visible_views` arithmetic moves with it).
- Multi-parent nodes (§3.2): one node reachable from several
  parents across instances; screen resolution is deferred to the
  enclosing instance (mechanism lands in P3/P4; the *table
  representation* lands here).
- Guards (§3.4): per-node preconditions over store fields with
  machine-readable refusals. The store doesn't exist yet — land
  the data shape and defer evaluation to P2. **Form is pinned
  (decided 2026-08-20):** a guard is a host-registered function
  per node (`fn(&Store) -> Result<(), Refusal>`) the framework
  calls at ask-time — no predicate DSL in the table, ever.
  Expressiveness stays in Rust; the framework only owns the
  one-list property and the refusal's delivery.

Acceptance: table-construction tests for each; the tier/area/
occlusion of every node queryable without instantiating any route
object.

**Phase gate:** ogham + lorekeeper + all three games compile and
test green against the re-export.

---

## Phase 2 — the store (repo: ogham, crate `structure`)

The order inside this phase matters; each WP builds on the last.

### WP-2.1 Schemas and derived reflection (§4.1, §4.3)

Extend the `#[derive(Editable)]` lineage
(`lorekeeper/editable-derive`, `editable-ogham`) so a schema
struct derives a **reflection**: field names, types, optionality,
declared defaults, declared grain (§5.5). Typed consumers read the
struct; the reflection is what document selections validate
against. Record shapes validate structurally, field-by-field
(§4.7) — never nominally.

Acceptance: a struct's reflection round-trips; a reflection
mismatch names the field; the derive covers every type currently
projected by `chrome::project`
(`untold_lore/ul-client/src/chrome.rs:706-1240` is the corpus —
grids, string lists, tuples, nested records, `Vec`s).

### WP-2.2 The store core (§5.1-5.5, §5.7)

Scoped fields keyed by route node; single-writer, equality-checked
sets (generalize `Chrome`'s compare, `chrome.rs:189-199`);
frame-transactional commit with the pinned intra-tick order
(outbox drain → producer sets → barrier → notify → rerender,
§5.4); subscribe (push, per-field) and read (pull, bulk) verbs;
scope lifetime = node presence on the path, with the three-rung
ladder (process/app-root scope, structural-node scope, view
scope). Guard evaluation from WP-1.2 activates here.

Acceptance: the FRP-glitch test (no consumer observes field A new
and field B old from one tick); ten sets → one notification; a
same-tick intent→producer→commit lands in that tick (celia's
tile-focus scenario, §5.4); scope state provably dropped when its
node leaves the path; an out-of-scope set is an error.

### WP-2.3 The write side (§4.4)

A scope publishes its accepted intents (typed, derived alongside
the schema); a document's raises validate at load against them;
a validated raise lands as an outbox action. This replaces the
`mpsc::Receiver<(String, Vec<RaiseArg>)>` seam
(`front/src/host.rs:64`) and the stringly decoding idiom
(`untold_lore/ul-client/src/chrome.rs:532-625` `intent_from_raise`
is the anti-corpus — its `text(0)? == "focus"` parsing and the
"RaiseArg has no as_bool" workaround at `chrome.rs:544-547` must
be unwritable in the replacement).

### WP-2.4 Validation, two grades (§4.1)

Selection errors **refuse**, named, at load and at every hot
reload (a hot-reload refusal rejects the new document without
tearing down the running instance — preserve the
`blank()`/`Chrome::failed` keep-the-front-door behavior,
`celia-client/src/client.rs:139-173`). Coverage drift **reports**.
Preserve the CI moment: ship a generic "load every shipped
document against its schemas" test harness that each consumer
instantiates — this is what lets the bespoke `.ogh`-string-parsing
guard tests delete in P6 without regressing the guarantee from
`cargo test` to first boot. Also validate the unread direction as
a report ("provided but read by nothing" — celia's dead root
`status` and arena `status` are the live examples).

---

## Phase 3 — the surface framework (repo: ogham) ∥ P4

### WP-3.1 Module system with transitive hot reload

The long pole for every document split. Records already import at
the schema layer (`runtime/schema.rs` "declared (or imported)
record"); needed: top-level `let` functions and records shared
across documents via import (`docs/internal/LANGUAGE.md:141,
307-313` states top-level-only import), and the hot-reload watcher
following the import graph — an edit to a shared module reloads
every mounting document. Regency's target split has a
`stationery.ogh` imported by two instance documents; celia's has
`paperwork.ogh` imported by three. A palette token edited in the
shared module must propagate on save.

### WP-3.2 Selection replaces `host_state` (§4.1, §4.6, §4.7)

The `host_state {}` block inverts to a selection against a
provider scope's reflection. Two properties are load-bearing:

- **Top-level binding (§4.6):** `select manor { hud, search, tip }`
  binds `hud` as a top-level name. Regency's thirty-two helpers
  read `hud.clock` unqualified today; with binding the migration
  touches zero helper bodies, without it it is a 3,000-line touch.
  This is the acceptance test: regency's `manor_screen` family
  compiles against a selection with no body edits.
- **Fragments (§4.7):** one selection stated once, validated
  against each providing scope at each mount (untold_lore's
  `sea_panel.ogh` under world root and editor).

`screen` blocks stay per-document (a selection is the consumer's
own contract declaration) even when the view function they call is
imported — celia's pattern: thin `screen "settings"` blocks in
three documents, one imported `settings_screen`.

### WP-3.3 Two visible views stack

ROUTING.md §13.5, unfixed and now load-bearing: consecutive
`Presence` children rendered by the outlet flow instead of
stacking, which breaks exit-prompts-over-workspaces the moment
`Occlusion::None` views migrate (untold_lore's `PromptRoute`,
`ul-client/src/routes.rs:767`). Fix in the surface framework
before P6U needs it.

---

## Phase 4 — the driver (repo: lorekeeper) ∥ P3

### WP-4.1 Merge RouterHost and the crossing compositor

`front/src/host.rs` (whole) + the two-tree slide compositor
(`app/src/lib.rs:1077-1112`, `UiSlide`/`UiCrossing` at
`lib.rs:1318-1330`) merge into one binding — **a standalone crate**
(working name `driver`; decided 2026-08-20), not folded into
`app`, so the minimal host trait (WP-4.5) is implementable
without the platform tier. The binding mounts one
`Ogham` instance per active instance root from the table's
document names (§6.1) and holds the one store. Keep verbatim:
the tick order's second resolve (`host.rs:198-223`) until §5.4's
barrier subsumes it; presses-swallowed-during-crossing
(`host.rs:329-333`); `app/src/routing.rs` stays platform-side
(ogham event semantics, per ROUTING.md phase 3's finding).
Delete: `own_ui`, `mounted_ui`, `ui_crossing`, `ui_departing`,
`brings_own_document`, `HostCx::project_root`/`project_views`
plumbing (§6.1). `FrontCx`'s getters dissolve in P5.

### WP-4.2 Draw slots, occlusion at the mount (§6.2)

Per-instance-root backdrop painter, declared at the mount:
signature receives world + store frame parameters via the read
verb, never the store's write surface; a continuous-animation
flag (the damage question — today celia's arena paints only
because `RouterHost::render` calls unconditionally); opaque vs
composites-over-painter as mount data. A ghosted instance retains
its draw slot (§5.6). Retire `Route::draw`/`occludes` — the model
is celia's `ArenaRoute::draw`
(`celia-client/src/routes.rs:200-213`).

### WP-4.3 The crossing generalizes (§6.4)

Extract `Sweep` from `untold_lore/ul-client/src/crossing.rs:59-260`
into the driver as the parameterized default passage. The binding
detects instance-root boundaries from the path delta (deleting the
game-side `drive_crossing` diff idiom); **mount-before-sweep** is
an invariant; per-edge override lands on `Departure` (promoted to
the instance tier); host-painted instances ghost as captured
composite frames (§5.6 — `Departure::Bleed`'s existing semantics,
`ogham/src/route/mod.rs:268-276`). Freeze becomes unsubscription
(§5.6): the binding unsubscribes the departing instance at
crossing start, drops it on settle; the parked-tree bookkeeping
(`ul-client/src/client.rs:2152-2187` `departing_library`) becomes
binding-owned.

### WP-4.4 Side channels and shared caches (§6.3, §6.5)

Anchors (`set_anchor` sites: `regency-client/src/client.rs:1319`,
`regency-client/src/manor.rs:6139`,
`lorekeeper/ogham_preview/src/main.rs:140`) and painter payloads
(regency's `DialHandle = Arc<Mutex<Option<DialPaint>>>`,
`regency-client/src/client.rs:202`) become binding-owned,
per-instance, name-validated channels. Image/font caches move to
the binding, shared across instances — today `set_image_root` is
per-`Ogham` (`celia-client/src/client.rs:156-159`) and celia's
roster art is 5.4 MB; a crossing must not re-decode on the
crossing frame (§6.5).

### WP-4.5 The minimal host trait (§8.1)

`HostCx` minus the game-flavored methods: render-context-ready,
window controls, tick-producers, gesture stream. `app` is one
implementation; port `ogham_preview` (already hand-driving a
single instance) as the second — it is the proof the trait
carries no game opinions. Nothing naming a session, save, or
capability may appear on it.

**Phase gate (P3+P4):** lorekeeper green; the three games still
green on compatibility shims; `ogham_preview` runs on the trait.

---

## Phase 5 — front becomes schemas + producers + views (repo: lorekeeper)

### WP-5.1 Published scope schemas

`MenuRow`, `SettingRow`, `SlotRow`, `PauseState`, `ConnectState`,
`LoadingState` (all in `front/src/*.rs`) become published schema
types with derived reflections and published intent vocabularies
(`menu`, `setting`, `back`, `join`, `slot`, `address`). Delete the
`record` blocks from `front/data/ui/front.ogh:17-33` — and note
for P6: the same shapes are hand-redeclared in all three games'
documents (`ul-client/data/ui/client.ogh:47-54`,
`celia-client/data/ui/client.ogh:62-105`,
`regency/data/ui/client.ogh:217`); those copies die in the game
migrations. Games `pub use` the types for their own producers.
Grants (§4.5): the game chooses which consumer classes see which
projection; extension is a game-side wrapper schema embedding the
engine's.

### WP-5.2 The front-of-house instance root (§8.2)

Title/connect/settings/saveload become views of an engine-provided
`foh` instance root mounting `front.ogh`; loading a small second
root. A game may replace the root's document **wholesale**
(celia's `menu.ogh` in Appendix A.1 does exactly this) — the
engine contribution that survives replacement is the schemas,
intent vocabularies, and producers, never the surfaces. Pause/settings/saveload additionally register as **view
nodes spliced under game-named roots** — `front::register`
(`front/src/lib.rs:198-215`) reshapes to exactly that and must not
assume root placement. The six routes' per-frame `update` bodies
(`title.rs:205-211`, `settings.rs:47-52`) become producers whose
equal sets the store swallows. `title_sub_screen` /
`FrontAction::Menu` (`front/src/lib.rs:147-159`) inverts: the
game's claim becomes a store field the title node's `resolve_child`
reads. Keep `MenuLabels` (`title.rs:35-88`) as producer config.

### WP-5.3 Services become producers

`drive_menu_music` (`services/src/front.rs:229-260`, memo bool at
`front.rs:132`) becomes a producer subscribed to the path's area
field — keeping the audio-bus read-back exception (§5.3) for the
actual play/stop calls. Sweep `services`/`shell-core` for other
polled-through-`FrontCx` state and convert to set-on-change.

---

## Phase 6 — consumer migrations (∥ per game)

Common shape for all three: build the tiered table → split the
documents → convert projections to producers → convert raises to
published intents → convert path-readers to subscriptions → delete
the ledger items → instantiate the generic load-validation harness
(WP-2.4). Each game lands as a small series of green commits, not
one megacommit. Appendices A/B hold the target tables and scope
partitions; Appendix C is the deletion checklist each game signs
off against.

### P6C — stargazer-celia-game

1. **Table + split** (Appendix A.1): roots `menu`, `lobby`,
   `arena` under structural `session`; documents `paperwork.ogh` /
   `menu.ogh` / `lobby.ogh` / `arena.ogh` per the split map in the
   audit (client.ogh line ranges: shared kit 24-52, 140-310,
   370-457, 781-789; menu screens 314-368, 459-489, 798-826;
   lobby 68-100, 497-713; arena 732-776).
2. **Hazard, checked in review:** `lobby_ui.focused`
   (`cx.rs:63-67`) scopes to `session`, never `lobby` — scoping it
   to the lobby silently loses pane focus across every match.
3. Root `host_state` deletes (headings become per-document
   literals; dead `status` keys at `cx.rs:403-409` and
   `client.ogh:834` delete). `project_root` (`cx.rs:384-410`)
   deletes. The `pick` handler's direct write (`routes.rs:79`)
   becomes intent-only — §5.4's same-tick commit keeps it
   zero-lag; verify by eye and by the WP-2.2 test.
4. Keep: `lobby.rs` projection + resolvers (already
   producer-shaped, unit-tested), `arena.rs`, all of `celia-core`.
   `tick_fight` keeps gating on `stage`, not path (§3.3 converse —
   do not "fix" this).
5. Crossings: menu→lobby (loading's last frame is the ghost),
   lobby→arena (deployment passage), arena→lobby (captured
   composite — predictor resets on stage change, `cx.rs:244-249`).
6. Rewrite `CLAUDE.md` — it still describes `chrome.rs`,
   `lobby::Chrome`, the `mode` string, and deleted guard tests.

### P6R — regency

1. **Table + split** (Appendix A.2): roots `foyer`, `table`;
   Wheel and Epilogue **promoted to views** (stage ↔ path becomes
   a bijection; `stage_route`'s three-way collapse at
   `routes.rs:49-55` deletes); pause a child view of the four
   playable table views; `settings` one multi-parent node;
   **saveload not registered** (caps-gated off,
   `client.rs:155, 1577-1579`). Documents `stationery.ogh` /
   `foyer.ogh` / `table.ogh` (split map: shared 303-330, 347-583,
   2614-2760; foyer 2308-2613, 2761-2774, 3221-3226; table the
   rest; records 15-286 delete).
2. Forced single-writer merge: `waiting` projected from two sites
   (`client.rs:587-598` and `1331-1348`) → one holding-scope
   producer.
3. `teardown` (`client.rs:886-921`) halves — scope-death replaces
   `lobby_ui.reset`, the manor panel resets, and the trailing
   `reproject_world`; what stays are the §5.3 external-state
   reactions (music fade, voice close, hosting shutdown).
   `wants_exit_to_menu` (`client.rs:374, 1614`) → outbox action.
4. Crossing: the bleed becomes the foyer↔table passage fed by the
   ghost composite (deletes `just_entered` +
   `render_menu_backdrop_offscreen`, `client.rs:1370-1376,
   1554-1567`; fixes the card-pop defect; creates the outbound
   transition). Intra-table page turns stay snapshot-fed
   (`prev_view`, `client.rs:1386-1399`) — world geometry is past
   the fence; only the trigger becomes a path subscription.
5. Typed intents: the colon-packed `menu("stash:item:container")`
   vocabulary (`client.rs:1010-1031`) splits into per-scope
   intents (`search`, `stash`, `pursuit`, `bots`, `leave`).
6. §5.5 cleanups: caret blink (`client.rs:1687-1689`) moves into
   the document; `Screen {w,h}` becomes a binding intrinsic.
   Music handover rides the crossing (`client.rs:162-165, 537,
   1970-1973`).

### P6U — untold_lore

The largest migration; sequence it as: scopes/producers → raises →
subscriptions → route table → crossings → deletions.

1. **Scope partition** (Appendix B.1): the 78 keys into ~13
   scopes. Watch the three deliberately-projected-while-down
   fields — `rooms` (`client.rs:3344-3348`), `sky_clock`
   (a *session* fact per the comment at `client.rs:3350`, not a
   sky-stance fact), `sea_panel`'s always-bound default.
2. Producers: `project` (chrome.rs:706-1240) deletes into
   reflection; `reproject`'s joins (item card 3243-3294, worn
   panes 3218-3234, bars 3300-3322) and `project_menu`'s
   (4575-4698) relocate as §5.7 producers; the scope-arm skeleton
   and `State::default()` overwrites delete.
3. Subscriptions: `drive_music`/`drive_backdrop`/`drive_crossing`
   memo-diffs delete (client.rs:2518/2279/2161); the hash-diff
   reconcilers (`sync_terrain_offer` 1031, `sync_session_map`
   1075, `sync_session_backstory` 1127) become subscriptions;
   `refresh_readiness`'s four hand-placed invalidation sites
   (9694, 1116, 1041, routes.rs:675) collapse to one subscriber.
4. Route table: four roots (`TITLE`, `ROSTER`, `WORLD`,
   `LIBRARY`) with the `area` attribute replacing `side_of`;
   rooms table → nodes + §3.4 guards (`Requires`/`Refusal`,
   rooms.rs:73-124, 296-406); `WORLD_SCREENS` +
   `brings_own_document` delete (routes.rs:97-110).
   **Behavior change, say it in the commit:** the world root
   re-mounts per session instead of catching up
   (client.rs:9997-10012; CROSSING.md §3 repeal).
5. Pause writes: the game writes "Editing {world}" into the
   engine's pause scope (client.rs:4650-4668) — this exercises
   the P5 grants/extension mechanism; `sea_panel` exercises §4.7
   fragments.
6. **WP-P6U-ED (decided 2026-08-20): two steps.** In this pass,
   the *minimal repoint only*: move `ul-editor` off the `shell`
   crate (whatever thin shim that takes) so P6S can delete
   `shell`. The full contract migration — the ul-editor family's
   instances becoming instance roots, `EditorShell::host_state`
   (176 lines at `ul-editor/src/shell.rs:1185-1361`) and
   `map_chrome.rs` (647 lines) becoming schemas, the reinjection
   loops (`ul-editor/src/client.rs:200-206, 310-315`) deleting —
   is a scheduled follow-up after P7, not part of this pass.
   APPLICATION.md §7 claims the ledger; this schedules it. Do not
   let the shim grow features: it exists to sever the `shell`
   dependency and nothing else.

### P6S — shell deletion (repo: lorekeeper)

After P6U repoints `ul-client` (Cargo.toml:67) and WP-P6U-ED's
minimal repoint moves `ul-editor` off `shell`: delete `shell/`
(ROUTING.md phase 8b as scheduled). `shell-core` stays.

---

## Phase 7 — the docs settle

- `lorekeeper/docs/ROUTING.md`: the amendment its header promises
  (route tier leaves the engine's orbit; instance tier; the
  store). Mark §13.4 superseded by WP-0.1/WP-2.4 and §13.5 fixed
  by WP-3.3.
- `untold_lore/docs/CROSSING.md`: axiom 4 half-repeal, §3 repeal
  (per APPLICATION.md's header).
- `untold_lore/docs/ROOMS.md`: `Presentation` as a reading of the
  node; guards moved per §3.4.
- `front` module docs re-cut for the schemas+producers+views
  shape; `AGENTS.md`s refreshed in every touched repo; celia's
  `CLAUDE.md` already rewritten in P6C.
- `APPLICATION.md` header: flip "not built" when the last phase
  gate passes.

---

## Appendix A — target route tables

### A.1 celia

```
/                        structural; child by SessionLevel
├─ menu                  ROOT → menu.ogh
│  ├─ title              view (default)
│  │  ├─ connect         view
│  │  ├─ settings        view  ← shared node
│  │  └─ saveload        view
│  └─ loading            view (while connect_status)
└─ session               structural; owns session scope (focused
   │                     character, stage, seats)
   ├─ lobby              ROOT → lobby.ogh
   │  └─ pause           view → settings (same shared node)
   └─ arena              ROOT → arena.ogh  (draw slot: fight painter)
      └─ pause           view → settings
```

### A.2 regency

```
/                        structural; session? → table : foyer
├─ foyer                 ROOT → foyer.ogh
│  ├─ title              view (default) → connect, settings
│  └─ loading            view
└─ table                 ROOT → table.ogh   (session lifetime)
   ├─ holding            view (default until first snapshot)
   ├─ seating            view
   ├─ wheel              view (promoted)
   ├─ manor              view (draw-adjacent: dial Canvas, tooltip anchor)
   ├─ epilogue           view (promoted)
   └─ pause              view (child of the four playable views)
      └─ settings        view  ← same node as foyer's
```

### A.3 untold_lore

Four roots as today (`TITLE`+loading, `ROSTER`, `WORLD`,
`LIBRARY`), `area` = {menu, world, library} with roster and world
sharing `world`; rooms become view/overlay nodes under `WORLD`
with `keys_reach_body` and §3.4 guards; pause subtree spliced from
`front` under every root. If WP-P6U-ED proceeds: `editor` and
`backstory_editor` become roots (area `library`).

## Appendix B — scope partitions

### B.1 untold_lore (78 keys → scopes)

| scope (node) | keys | producer today (anchor) |
|---|---|---|
| app-global | `launch_fade`, `heading`, `status` | client.rs:2316, 4585 |
| title root | `menu`, `connect_*`, `settings`, `slots`, `slot_action*`, `continue_world` | project_menu 4586-4637 |
| worlds view | `worlds`, `world_saves`, `world_heading`, `creating_world`, `new_world_name`, `can_create_world`, `world_hint` | 4619-4631 |
| pause view (engine scope + game extension) | `heading`, `menu` | 4638-4668 |
| session root | `world_name`, `clock`, `players`, `entries`, `composing`, `standing`, `needs`, `athletics`, `saved_note`, `feed_note`, `save_name`, `naming_save`, `can_record`, `can_save`, `sky_clock`†, `sky_hour_now`† | play::project via reproject 3144-3165 |
| pre-body view | `roster*`, `creation_*`, `walked`, `can_seat/accept/step_back`, `reviewing` | creation::project 3119-3137 |
| hub view | `readiness_note`, `checks`, `hub_action*`, `can_author` | worldbuilder::project 3125-3132 |
| HUD view | `prompt_verb`, `bar_main/off`, `bar_slots`, `tail_rows` | 3186-3190, 3300-3322 |
| menu-room view | `menu_tab`, `chest_pane`, `bench_pane`, `inventory_rows`, `equipment_rows`, `worn_panes`, `item_card`, `card_puts`, `chest_rows`, `chest_label`, `recipes`, `skills`, `char_story` | 3194-3341 |
| sea stance | `sea_panel`, `sea_duration`, `sea_dirty` (fragment, §4.7) | 3177-3181 |
| wardrobe stance | `wardrobe`, `wardrobe_clothed`, `can_cancel` | 3171-3176 |
| rooms panel | `rooms` (projected while down — session-scope candidate) | 3344-3348 |

† sky fields live on the session per client.rs:3350's own comment.

### B.2 regency

app root: `Settings`, `session_note` (must outlive the table —
written at teardown, read on the title card, client.rs:294,
886-892, 1446-1449). foyer: `MenuCard`, `Connect`. table:
`phase`, clock, voice, `Debug`. holding: `Waiting`. seating:
`Lobby`/`Sheet`. manor: `Hud`, `Hotbar`, `Toast`, `Hands`,
`Notice`, `Search`, `Wheel`(dial lane), `Tip`. pause: engine rows.
wheel/epilogue: empty on day one.

### B.3 celia

session (structural): stage, seats, focused character. lobby:
`LobbyView` **flattened to top level** (one nested `view` field
forfeits per-field invalidation — the record-grain lesson). arena:
empty or deletes. menu views: engine scopes (six of eight scopes
are the engine's).

## Appendix C — deletion ledger (end-state check)

Run per repo when its migration lands; every item must be gone or
the ledger says why not.

**untold_lore:** `chrome::project` + helpers (~590 ln);
`every_declared_key_is_projected`;
`every_declared_raise_reaches_a_handler` (+ their .ogh string
parsers); `UlGame::project_root` / `project_views` / `mounted_ui`;
`RAISES` / `ENGINE_RAISES` + mpsc loop; `crossing::side_of`;
`drive_music`/`drive_backdrop`/`drive_crossing` memo-diffs;
`intent_from_raise`; `WORLD_SCREENS`; `brings_own_document`;
`Held`; both `host_state {}` blocks as provider declarations.

**regency:** record block client.ogh:15-286; `host_state {}`;
schema-conformance test (lobby.rs:1052-1101); raise/screen guard
tests (client.rs:2026-2059); the 16 `!=`-guard sites;
`panels_dirty` / `take_panels_dirty`; `wants_exit_to_menu`;
`last_page` edge memory; `just_entered` +
`render_menu_backdrop_offscreen`; `stage_route`; `TableRoute.paused`
claim choreography; `RAISES`; the colon-packed menu vocabulary;
the saveload stub screen.

**celia:** `HostCx::project_root`; root `host_state`; `RAISES` +
mpsc registration; guard tests
(`the_shipped_document_declares_exactly_the_registered_screens`,
`every_declared_raise_has_a_handler`, wire-connectivity half of
`every_button_reaches_a_route`,
`pause_does_not_survive_a_trip_to_the_menu`); both `paused` claims;
per-frame `lobby_view` recompute; dead `status` keys; the
hand-mirrored `MenuItem`/`SettingRow`/`SlotRow` records.

**lorekeeper:** `shell/` crate; `Chrome::last` as a distinct
mechanism; `Route::{read_state, draw, occludes}`;
`own_ui`/`mounted_ui`/`ui_crossing`/`ui_departing`;
`FrontCx` getters; `menu_music_playing`; the `RaiseArg` mpsc;
record blocks in `front.ogh`.
