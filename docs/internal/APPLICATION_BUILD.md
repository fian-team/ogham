# Ogham — The Application Build

> **Status: implementation plan for `APPLICATION.md`.** Written
> 2026-08-20 from the four-repo consumer audit. Audience: an
> orchestrating agent driving subagents through the entire suite of
> changes. `APPLICATION.md` is the design record — this document
> never restates its axioms, only cites them (§n.n references are
> into APPLICATION.md unless prefixed with a repo doc name).
>
> **Build status lives in §0.5, updated as packages land.**

---

## 0.5 Build status

Last updated 2026-08-21, during Phase 7.

| package | state | commits |
|---|---|---|
| WP-0.1 | **landed** | ogham `dc30634`, lorekeeper `b9d0dc5` |
| WP-1.1 | **landed** (acceptance amended, below) | ogham `6c8c847` + consumer lockfiles `d0d689a` / `1508f05` / `0eb1a95` / `7f29d20` |
| WP-1.2 | **landed** | ogham `fa7092e` |
| **Phase 1 gate** | **passed** 2026-08-20 | — |
| WP-2.1 | **landed** | ogham `8f76f9c` + `e0dca27` (parser cut), lorekeeper `cb33238` + `f5dbb6b` |
| WP-2.2 | **landed** | ogham `a21c05e` |
| WP-2.3 | **landed** (+ the at-mount fix) | ogham `c04b25a`, `dd375c8`; lorekeeper `d20c774`, `0f51ba6` |
| WP-2.4 | **landed** | ogham `d292cea`, `ba65ae2` |
| **Phase 2 gate** | **passed** 2026-08-21 | — |
| WP-3.1 / 3.2 / 3.3 | **landed** | ogham `6c46c7e`, `e8344ec`, `1792a89`, `8f57cba` |
| WP-4.1 – 4.5 | **landed** | ogham `03d895f`, `ac04252`; lorekeeper `34b13d5`, `027a320`, `5d8ee11`, `2489d8f`, `953adaa`; lockfiles regency `19cfb4b`, celia `dedec38` |
| **P3+P4 gate** | **passed** 2026-08-21 | — |
| the contract crate | **landed** (maintainer-ruled, below) | ogham `e4397cc`, `a65b886`; lorekeeper `8a0bc4c` |
| WP-5.1 / 5.2 / 5.3 | **landed** (three amendments, below) | lorekeeper `94ae5a3`, `d073feb`, `d7e61be`, `5bb8fa6`; ogham `80f0ae3`, `5279a1f`; lockfiles regency `90191b1`, celia `aee27b4` |
| re-seed on reload | not started | — |
| re-seed on reload | **landed** | ogham `be0779c`; lorekeeper `338d2ca` |
| **P6C celia** | **landed, complete** | celia `7b93b5e`, `85bc49f`, `fa4abdc`, `41cd2f9` |
| **P6U untold_lore** | **landed, and a live regression is open against it** (below) | ul `1393641` … `2beffd0` (21 commits) |
| **P6R regency** | **landed** | regency `7a95348` … `c16237e` (13 commits) |
| **driver gaps package** | **landed, all five** | lorekeeper `8919ed5`, `b883b67`+`14094f9`, `e280e0c`, `2c13ad7`+`2e7640f`, `9b6053e`+`5f8111c`; ogham `9b6053e`, `2c13ad7`, `b883b67`, `1809915` |
| **P6S shell deletion** | **landed** | lorekeeper `6f00d42` |
| **P6 gate** | **one criterion not met** (the `ogham → structure` edge, below) | — |
| P7 the docs settle | **in flight, stopped short of the header** | lorekeeper `ea39cf1`, `8b296e7`, `bde48ae`, `bd1b2d3`, `0dc5341`, `5d3370a`; ogham `00253bd`; regency `1ca13a8`, `43bbf44`; celia `ddf6771`; ul `3ac12c4`, `91f6709`, `00c82bb` |

**Phase 1 gate, as run:** ogham, untold_lore, regency and
stargazer-celia-game all green; lorekeeper green except the baseline
red below. Consumers compile **unchanged** against the re-export.

**Baseline failures that are not this build's** (measure against
these, not against zero — the standing rule in §0):

- `lorekeeper render/tests/prop_load.rs`
  `every_slot_binds_and_flags_exactly_one_thing` — red since the
  skinned-shadow work landed (set 2 now binds 7 and 8; the test
  still expects `[0..4]`). Maintainer's in-flight work, untouched.
- `lorekeeper cook` `generated::tests::a_generated_source_reads_with_no_source_tree`
  — fails in full-workspace runs only, passes under
  `cargo test -p cook --lib`.
- ogham clippy/fmt debt. Deltas at the Phase 1 gate: clippy 110→108,
  fmt diffs 26→24 (both improved; nothing added).

**Pre-build repairs** (done before WP-0.1, so the baseline was a
real one): in-flight work committed in lorekeeper (`32bc6dc`,
`a94f0fa`) and untold_lore (`6215880`); regency and celia did not
compile against the engine's current API and were repaired
(`ae7a6dd`, `ae15801`) — `FrontConfig::manual_slots` had been
replaced by directory discovery, and `WindowControls::set_cursor`
now takes a `CustomCursorSource`, so regency re-rasterizes its
crosshair per hover swap instead of holding minted handles. regency
and celia were working on `routing` branches; both fast-forwarded
to `main` per §0's single-branch rule. **Nothing has been pushed.**

### The WP-1.1 acceptance amendment (decided 2026-08-20)

WP-1.1's two acceptance criteria are mechanically contradictory:
Rust has no way for `ogham` to `pub use` a `structure` item without
declaring the dependency, so "`cargo tree -p ogham` shows no
structure" and "consumers compile via the re-export" cannot both
hold. This document's own text prescribes the re-export ("Until P4
lands, `front::RouterHost` keeps working against a thin
re-export"), so the edge is prescribed with it. Resolution:

- `cargo tree -p structure` shows no ogham — **holds absolutely**,
  and it is the load-bearing half of §2 (the structure framework
  depends on nothing of the surface framework). The crate has an
  empty `[dependencies]`.
- The `ogham → structure` edge is **declared scaffolding**, marked
  at the dependency line and in `route/mod.rs`'s module doc, and
  the "`cargo tree -p ogham` clean" check **moves to the P4 phase
  gate** — where WP-4.1 deletes ogham's route remainder anyway.

A second finding sharpened the same seam: the `Route` trait cannot
leave ogham today at all, because its signatures concretely name
surface types (`read_state` returns `Value`s, `RouteEvent::Input`
carries a widget event, `own_ui` names `Ogham` itself) and four
repos' `impl Route<Cx, A>` blocks repeat them. That is coherent
with the plan's end state — `read_state` dies into the store (P2),
`own_ui`/`draw` retire in WP-4.1/4.2 — so the trait, `RouteEvent`
and `chrome.rs` stay ogham-side as scheduled scaffolding.

Consequently WP-1.1 moved the router's **walk** as well as the
table and outbox (rather than the minimal table+outbox move), so
that WP-2.2's guard evaluation lands inside a structure-owned walk
instead of being wired back through ogham's router. The walk is
generic over a `Node<Cx, A>` bridge trait carrying the nine
`Route` methods whose signatures name no surface type; ogham's
`Router` is a newtype over it, keeping only `event` and `draw`
dispatch. The bridge is scaffolding with the same P4 death date.

### What P2 inherits

`structure/` (workspace member, `publish = false`, zero
dependencies, 23 tests) holds: `table.rs` (`RouteTable`, `Tier`,
`TableError`), `router.rs` (the walk + `Node`), `outbox.rs`,
`guard.rs`, and the vocabulary in `lib.rs` (`RouteId`, `Area`,
`Handled`, `Occlusion`, `Escape`, `Departure`, `RaiseArg`).

WP-1.2's table API, for the packages that build on it:

- Declarations: `at_root`, `under`, `mounts(id, document)`,
  `structural`, `in_area`, `default_area`, `occludes`, `guard`.
- Queries answered **from the table alone**, no route object:
  `tier_of`, `document_of`, `area_of`, `area_of_path`,
  `enclosing_instance`, `occlusion_of`, `declared_occlusion`,
  `guard_of`, `parents_of`, `children_of`.
- `Tier` defaults to `View` (the un-migrated common case);
  `InstanceRoot { document }` and `Structural` are declarations.
- `Area = &'static str`, with `default_area` preserving
  untold_lore's catch-all property; declaring areas without
  choosing a catch-all fails at startup, as does an area on a view.
- Guards: `guard.rs` pins §3.4's form — `type Guard = fn(&Store)
  -> Result<(), Refusal>`, a plain fn pointer, never a predicate
  DSL. `Store` is a crate-private placeholder **WP-2.2 fills in
  under the same name**, so registered guards keep compiling; it is
  unconstructible outside the crate so no host can grow an
  ask-then-mount of its own first. `Refusal` carries one authored
  sentence (§3.4's "written once, read everywhere").
- `validate()` catches, at startup: unknown parent, duplicate id,
  cycle, no root, attribute-on-unknown id, and contradictory
  double declarations (two guards on one door, instance-root-and-
  structural, an area declared twice).

### Phase 2, as it landed

**Phase 2 gate, as run (2026-08-21):** ogham 810 passed; untold_lore,
regency and stargazer-celia-game all green; lorekeeper green except
the one baseline red above. `cargo tree -p structure` still one line —
the crate reached the end of the store with an empty `[dependencies]`.
`structure` went 23 tests → **101**.

**Three properties are now held by construction rather than by
convention**, which is worth recording because each replaces a
discipline the audit found being hand-maintained:

- **§5.4's intra-tick order is enforced by the borrow checker.** The
  outbox drain happens inside `Store::tick` before the producer
  closure runs; a set is reachable only through a `Writer`, a
  `Writer` only through the `Barrier`, and the `Barrier` only inside
  that closure. The `Barrier` holds `&mut Store` for its whole life,
  so no consumer can read while producers run. The FRP glitch is not
  guarded against — it is unrepresentable.
- **§4.4's stringly decoding is unwritable.** `Raise` exposes
  `name()` and `parameters(intent, arity)` and no way to reach an
  argument before naming an intent, so `text(0)? == "focus"` dispatch
  has no expression. `Args::take::<T>` takes no index and moves a
  cursor, so re-reading argument 0 is not a rule broken but a call
  that does not exist; `bool` is one of its types, so the
  missing-`as_bool` workaround has nothing to work around.
- **§4.1's declared at-mount value cannot disagree with the
  schema.** The derive emits the field's literal into
  `Field::starting_at` and into the at-mount value in one pass, and
  `Store::provides::<T>(scope)` no longer takes a value. The old test
  fixture was itself an instance of the bug — `Session::reflect`
  declared `stage = "lobby"` while `Session::default()` gave `""`.

**One acceptance criterion was read down, deliberately.** WP-2.1
first shipped `Kind: FromStr` beside `Display` to satisfy "a
struct's reflection round-trips". Nothing in the design can reach a
parser — providers own schemas as Rust types, the derive is the only
thing that builds a `Kind`, and every validator is in-process and
typed — so the reading half was cut (`e0dca27`) and the round trip
restated as what survives it (`f5dbb6b`): every declared field
reaches the reflection under its own name, resolves through
`field_at`, and prints.

**A grade change P6 must plan against.** §4.1 names "a screen no node
reaches" and "a published intent no shipped document raises" as
coverage drift, so WP-2.4 grades both as **reports**, not errors.
Today three games assert on exactly those with `.expect()` and fail
CI. The framework grades the finding; **the consumer decides whether
a report is fatal for it**, by asserting on `found.reports()` when it
instantiates the harness. Every game migration that deletes a
bespoke guard test must carry that assertion across, or the
guarantee silently weakens from "CI fails" to "a line of output".
The affected tests are named per game in WP-2.4's coverage list
below.

**What P6 does not inherit from the harness**, and must answer
another way: the *path* half of celia's `every_button_reaches_a_route`
(the harness asks whether some scope in a document's mount accepts an
intent; it cannot ask whether the accepting scope sits on the path a
particular button is drawn on — the document does not say which
screen raises what), and `pause_does_not_survive_a_trip_to_the_menu`,
which is a scope-lifetime property whose successor is WP-2.2's
"scope state dies with its node", not the contract harness.
Appendix C lists only the wire half of the former for deletion, so
this is consistent — but the remaining half stays until P4's driver
can answer it.

### Phases 3 and 4, as they landed

**P3+P4 gate, as run (2026-08-21):** ogham 852 passed; lorekeeper
green except the one baseline red; regency 54 and celia 49 green
**with no source edits at all** — only lockfile bumps;
`ogham_preview` runs on the four-method `driver::Host`.
untold_lore was not gated on: it is mid-surgery in the
maintainer's tree and does not compile
(`ul-core/src/server.rs`), which is unrelated to this build.

**Three defects were found that the plan does not list**, each of
which would have shipped looking correct:

- **The import walk was three readers disagreeing.** The compiler
  pre-scanned *direct* imports for `let` names only; the schema was
  handed an empty import map by every caller in the crate, so a
  `record` could not cross an import at all; and the watcher was
  built once at mount and never rebuilt — contradicting ROUTING.md
  §13.4's own claim that reload rebuilds it.
- **WP-2.4's hot-reload gate had a hole.** It built the candidate
  schema without imports, so a document declaring a field at an
  imported record shape produced an `Err`, the candidate became
  `None`, and **the gate was skipped entirely**.
- **Stacking two views would have taken the wrong click.**
  `FlexWidget::handle_event`'s pointer walk is front-to-back,
  first consumer wins — which a flow never has to decide, because
  its children do not overlap. Stacked children overlap exactly, so
  the workspace *underneath* would have taken every press aimed at
  the prompt drawn on top. WP-3.3 reverses the walk when stacking.

Also fixed in P4, unlisted: a path change *within* the arriving
instance during a passage cancelled the sweep, so a pause or a card
one frame in would snap the picture.

**§4.6's binding property is proved, not asserted.** WP-3.2's
acceptance vendors regency's real in-manor HUD family verbatim from
`client.ogh` and compiles it twice — once against the real 21-field
`record Hud` plus `host_state {}`, once against `select manor {
hud };` — asserting the two files are byte-identical after the
declaration. If the selected arm needed one edit the two arms could
not share the file.

### The dependency-edge ruling (decided 2026-08-21)

Phase 1 deferred "`cargo tree -p ogham` shows no structure" to the
P4 gate. **It cannot be met there**, and P4 correctly stopped rather
than forcing it. Two things held the edge:

1. `ogham/src/route/` — scaffolding as scheduled, but three games
   hold ~90 `use ogham::route::{…}` sites and their own `impl
   Route<Cx, A>` blocks, and ogham cannot re-export from a crate
   that depends on it. **This half of the criterion moves to the
   P6 gate**, where the `impl Route` blocks are rewritten anyway.
2. `ogham/src/contract.rs` — **not scaffolding.** WP-2.4 put
   document loading in `ogham` deliberately, so the harness would
   need no platform tier.

**Ruling on (2):** moving `contract.rs` into `driver` is rejected —
a consumer's `cargo test`-time document check would then link
`app`, `winit` and Skia, forfeiting the exact property WP-2.4 built
it for. Amending §2 is rejected — §2 is the spine. But §2 says
*only the binding depends on both*, not *there is exactly one
binding crate*, and `contract::Documents` needs the parser and the
store, so **it is a binding by §2's own definition**. It became its
own crate (`ogham/contract/`, `cargo tree --depth 1` = `ogham` +
`structure`, nothing else). `src/route/` is now the sole rider on
the edge, and the manifest says so.

`Chrome` had to split to make this work, which the ruling did not
anticipate: `ogham` keeps the mechanism (`Chrome::frame_gated`,
`Chrome::refuse`) and `contract` supplies the question (`trait
Checked`, with `check`/`frame_checked`). Call sites are
byte-identical plus one `use`.

### The leaf-depth gap, closed in the same package

The inversion cost a guarantee, and WP-3.2 flagged it precisely:
with `host_state {}` a provider renaming `Hud::clock` refuses at
load; with `select manor { hud }`, `hud.clock` read `Void` and
nothing said so. That is **a false expectation** — the one thing
§4.1 promises never to produce — and untold_lore's
`every_declared_key_is_projected` only deletes if the harness holds
what that test held.

Closed in the harness, the one place holding both the AST and the
reflection: `ogham` collects every member-access path off a
top-level bound name onto `ModuleSchema.reads` (grading nothing —
grading needs the reflection), and `contract`/`structure` resolve
each path with `Kind::field_at`. Because both the harness and the
reload gate read a `ModuleSchema`, **both moments get the check
with no API change**.

Grades, deliberately: a missing leaf **refuses** (`Unreached`); a
path that steps into a list, map, union or back-edge is **silent**,
not a report — §4.2 makes a collection one field in v1, so this is
the boundary of what is checkable, and a report here would fire on
every correct list read in every document and be switched off
within a week; a read off a `fn` parameter, `let`, `state` or `for`
variable is never collected at all, because a name the document
bound is its own. A false refusal would be worse than the gap.

### Phase 5, as it landed — and three amendments to this document

Green: lorekeeper (only the baseline red), ogham 853, regency and
celia **with no source edits at all**, lockfiles only. `front`
went 17 → 24 tests.

**Amendment 1 — `loading` is a view, not "a small second root."**
WP-5.2's prose says a second root; Appendices A.1 and A.2 both draw
it as a sibling view of `title` under `menu`/`foyer`. **The
appendices win** — they are the target tables three migrations are
written against, and a sibling view keeps loading out from under
wherever you came from while letting it share the front-of-house
document. WP-5.2's sentence is wrong; read it as the appendices
draw it.

**Amendment 2 — §4.5's "extension is a game-side wrapper schema
embedding the engine's" is wrong for this design.** Checked against
untold_lore's live case, which turns out **not** to write into the
engine's pause scope at all: it projects its own heading and builds
its own row list, including an Exit-to-World row the engine's list
has no place for. Two findings:

- A wrapper schema embedding the engine's is *a different type*,
  and `Store::producer::<T>` is typed — the engine's producer could
  not write into it. One scope, one schema.
- What extension actually is: **the game claims the fields the
  engine's producer leaves alone, and declines the producers whose
  facts it owns.** Field-level claims (WP-2.2) make this free —
  two disjoint claims on one scope, checked at startup. The engine
  uses it internally: `Producers::title` claims `heading` and
  `menu` and deliberately leaves `sub_screen` unclaimed, because
  that claim is the game's.

A game adding a genuinely *new* field to an engine scope is the one
case neither mechanism covers; that field goes on a scope of its
own on an adjacent rung, which nearest-first fragment resolution
already makes readable from the same document.

**Amendment 3 — `menu_music_playing` stays; strike it from
Appendix C.** It is not a value-diff idiom. It is the audio bus's
read-back stand-in, which §5.3 explicitly preserves, and the bus
genuinely cannot answer — a track whose file is missing never
starts, so a real read-back would retry and log every frame. The
*fact* moved to `House::menu_music`; the flag is what §5.3's
exception is for.

**Ruled, not an amendment:** the stock pause *surface* was cut from
`front.ogh`. Pause is a view under the game's roots, so its screen
resolves against the game's document, and selecting `pause` from
the front-of-house mount is a hard `Unmounted` refusal. Schema,
vocabulary and producer stay — which is exactly what §8.2 says
survives wholesale replacement: *the schemas, the intent
vocabularies and the producers, never the surfaces*. All three
games write their own pause view today.

**Forced by §4.6, and worth knowing before the migrations:** a
selected field binds a *top-level* name, and ogham refuses two
bindings of one name in one document — so three `rows` fields in
one front-of-house document is a parse error. Hence
`TitleState.menu`, `SettingsState.options`, `SaveLoadState.slots`.
Pause keeps `rows`/`over`, because pause is drawn by the game's
in-session document.

**What Phase 4 did not reach, and Phase 5 had to add.** There was
**no store→document path at all**: the binding projected
`Route::read_state` and nothing else, and nothing in `structure`
can hand a dynamic consumer a value (turning a scope struct into
an `ogham::Value` names both frameworks). `Binding::projecting`
and `driver::facts` are that path. Seeding also turned out to be
**mandatory, not optional** — a selected name is an ordinary
host-state key and a module's top level runs at *construction*, so
an unseeded selection is `UndefinedVariable` at mount: the document
does not draw blank, it fails to load.

**Open wart, scheduled before P6:** the seed rides `RuntimeConfig`,
so a hot edit that *adds* a selected name has nothing bound for it
and the reload is refused as broken until restart (existing names
survive via `carry_host_state_into`). That is precisely the loop
three migrations are about to live in. Closing it needs the binding
to re-seed on reload, which needs the reload gate to hand back the
candidate schema.

### Phase 6 — what the migrations proved, and what they found

**P6C (celia) is complete and green**: four documents, 50 unit
tests (was 43), `celia-core` byte-identical. Two design promises
held under a real migration. The **zero-lag pick** —
`a_pick_moves_the_pane_in_the_tick_that_raised_it` raises with no
`tick` between and the committed pane has already moved; `confirm`
resolves *inside* the barrier after the focus is set, so
pick-then-confirm in one tick signs for the tile actually clicked.
And **scope death replaces reset** — the focused file survives the
trip to the arena and back, then the next session starts at the
declared at-mount value because the node left the path.
`LobbyUi::reset()` is deleted.

**§0.5 amendment 2 is confirmed in practice.** The engine claims
pause's `rows` and leaves `over`; celia claims `over` and writes
its own heading there. No wrapper schema anywhere.

**The harness earns its keep during a split**: celia's caught three
truncated function bodies — a missing `};` in each of three
screens — as named errors rather than blank screens at runtime.

**P6U found a live bug of the exact class this build exists to
prevent.** `world.ogh` declares `map_sea_color(string, int,
string)` — the third argument is the *text* a colour input
committed — and `intent_from_raise` read it with `num(2)?`, which
returns `None` for a `Str`. **Every RGB edit made from the
in-world sea stance was silently dropped**, for as long as the
panel has existed. The editor's own handler parses the string, so
only one of two surfaces was broken and neither said so. A typed
vocabulary surfaced it within the first hour.

**WP-P6U-ED's premise was wrong.** The plan scheduled a two-step
editor migration because `ul-editor` depended on lorekeeper's
`shell` — but every `shell::` in that crate is `crate::shell`, its
own module. The plan read a module path as a crate path. The real
last consumer was `ul-client`, and all five uses were re-exports
repointed at `shell_core` and `front::boot`. **No shim, zero new
code. P6S is unblocked.**

### Five framework gaps the migrations found

Four from celia, one from untold_lore. All real; collected into a
driver package rather than patched mid-migration, because two
agents were live in those crates.

1. **`driver::Runner` never calls `tick_store`.** A game with
   producers of its own cannot use `Runner` at all. Celia
   re-implemented the frame (~70 lines) to put the barrier between
   `deliver` and the second resolve. Every migrating game will.
2. **`Binding::supply_at(root, chrome, slot)` does not exist** —
   **the most consequential of the five.** `Binding::supply` hands
   the binding a pre-built instance, but only for the *unnamed*
   one, and `Instance::new` is not `pub`. So the instance tier is
   all-or-nothing per root: a host either declares a document name
   and gives up ownership, or keeps ownership and gets no instance
   at all. Every game with a host-owned workspace is stuck there —
   untold_lore has three. This one method is the whole gap between
   the `mounted_ui` shim and `mounts(id, "…")`, and it also makes
   `in_area` reachable, since `in_area` refuses on a
   non-instance-root.
3. **`Binding::mount_for` refuses on unpublished structural
   ancestors** — it names every ancestor unconditionally, so a
   structural node publishing no scope returns an `Unpublished`
   *refusal* about a node doing exactly what a structural node is
   for. Needs a `reflection()`-or-`intents()` filter, which needs
   a `&Store`.
4. **No typed route access out of the walk.** `Router::get` hands
   back `&dyn Route` with no downcast, so a producer needing a
   route's own state (celia's case: the half-typed address, which
   is the connect route's own per ROUTING.md §7.1) reads it back
   through the `read_state` shim.
5. **`Route::child_popped` carries neither store nor outbox**, so a
   claim the *router* releases has no zero-lag path into a scope.
   Both games' `paused` claims therefore stay route state, and
   that Appendix C item does not land whole in either.

Minor: `Assets::space()` reads `config.project_root`, `None` unless
the host sets it, so a split document's mount seed silently sees
nothing. Both games set it explicitly.

### P6U's second stop: the passage rule

`driver::crossing::is_passage()` is `from.is_some() &&
to.is_some()`, so a boundary where either side is not an instance
root is not a passage — no ghost, no sweep, and `restand` drops the
departing instance. Consequently **every partial jump regresses one
of untold_lore's two crossings**: name the front of house and
`session` as instance roots and `menu ↔ library` becomes a cut
(the library is not one); leave the front of house unnamed and
`session → menu` becomes a cut instead. The menu↔library sweep is
this month's deliberate work (`6215880`), so neither is acceptable,
and both alternatives — regressing it, or demoting the library to a
view, which contradicts CROSSING.md axiom 1 — were correctly
declined. **Gap 2 is what unblocks it.**

The library cannot be an instance root today because `mounts()`
makes the binding build the `Chrome` from `Assets`, while the
library's tree is `ul_editor::EditorClient`'s own `Ogham` — it owns
its runtime, registers ~40 handlers at construction, and
hot-reloads itself. Handing that over *is* the editor contract
migration, which WP-P6U-ED explicitly excludes.

The `area` attribute is blocked by the same rule (`in_area` refuses
on a non-instance-root), so `crossing::side_of` survives this pass,
reduced from three roots to two. Appendix A.3 anticipated this — it
conditions the library area on WP-P6U-ED proceeding.

**P6U's table redesign did land** (`c029e9b`): `session` is a node
at root with `roster` and `world` as views, so **the seated
hand-off is no longer a root change** — it is a walk within one
instance, which is CROSSING.md axiom 2 held by the table instead of
by every reader naming both roots (amended in place, dated). It
also resolved B.1's second deviation: `Hud` split onto
`Scope::Node(WORLD)`, which only became sayable once a node existed
whose lifetime is the session.

### The rest of Phase 6, as it landed

**P6R (regency) is complete and green**: two instance roots under a
structural `house`, five pages as views, three documents, 50 tests. The
promotions Appendix A.2 asked for all held — `wheel` and `epilogue` are
views, so stage ↔ path is a bijection and the page-turn edge detector
is deleted; `settings` is one multi-parent node; `saveload` is not
registered rather than drawn dead. The colon-packed
`menu("stash:item:container")` vocabulary split into five per-scope
intents, which is where two of this build's five live bugs surfaced: a
settings adjustment that had been dead since the shell went, and two
buttons whose raises reached nobody.

**P6U (untold_lore) landed the rest of its sequence** — the areas as
table data, both documents selecting per scope, the rooms as guarded
nodes, the readiness checklist as one subscription, and pause as a fact
of the node it is over. `chrome::project` is deleted, five hundred and
eighty-one lines of it.

> **A live regression is open against P6U, reported 2026-08-21 by the
> maintainer**, and it is the reason this document does not yet say the
> build is done. In the world view nothing renders — a black screen —
> the escape menu opens and cannot be closed by key or click, and audio
> keeps playing. Under investigation; fixes are expected to land in
> `ul-client`. **The suite is green.** That gap between a green
> `cargo test` and a black screen is the most useful thing this build
> has to say about its own gates: every check this build added answers
> a question about *contracts* — does a selection name a field that
> exists, does a raise reach a vocabulary, does a scope mount when its
> node joins the path — and none of them answers whether a frame was
> drawn. Three games have entrance/render tests that do drive a frame;
> untold_lore's `tests/launch.rs` drives the title's chrome, not the
> session's. What is missing is named rather than guessed at: nothing
> renders a *world* instance under a real path and asserts it painted.

**P6S is done in one commit** (`6f00d42`): `shell/` deleted, 5,168
lines, `shell-core` untouched. WP-P6U-ED's premise had already been
found wrong — every `shell::` in `ul-editor` was `crate::shell`, its own
module — so there was no shim to write and nothing to unpick.

### The driver gaps package, as it landed

All five closed, and two of them turned out to be one shape:

1. `Runner`'s frame gained the barrier and two side-channel seams
   (`8919ed5`), so a game with producers of its own stops re-writing
   seventy lines of the frame. Both regency and celia deleted their
   copies (`f400d63`, `6d19b41`).
2. `Binding::supply_at` exists (`14094f9`, `b883b67`) — the most
   consequential of the five, as predicted. An instance root may leave
   its document to the host and is an instance root either way, so
   `in_area` became reachable for a host-owned tree and untold_lore's
   library stopped being the thing that could not be a root.
3. `Binding::mount_for` filters unpublished ancestors (`e280e0c`), and
   the mount is assembled from the table, the store and the assets, so
   no consumer builds one by hand any more.
4. Typed route access out of the walk (`2c13ad7`, `2e7640f`): a route
   comes back at the type that registered it, and celia's half-typed
   address reaches the scope that publishes it without the `read_state`
   shim.
5. `Route::child_popped` carries the store and the outbox (`9b6053e`,
   `5f8111c`), so a claim the router releases reaches a scope in the
   frame it was released.

**(5) did not land the same way in both games, and the difference is
worth recording.** celia's `paused` claim moved into the scope of the
node it is over (`f3c4a16`), and untold_lore's went with it
(`2beffd0`, taking four flags that had deferred a release). **regency's
did not**: it is still `Rc<Cell<bool>>` shared between the table root
and its pages (`routes.rs`, `PauseClaim`). That is a deliberate stop,
not an oversight — regency's pause hangs under four sibling views that
each need to read and clear it, and moving it needs the claim to live
on the *table* root's scope while the views that set it are its
children, which is a shape the ladder reads but the release path did
not yet write. Appendix C's `TableRoute.paused` item therefore does not
land whole.

### The P6 gate: one criterion did not come true

`cargo tree -p ogham` showing no `structure` was deferred from the
Phase 1 gate to the P4 gate, then ruled from the P4 gate to the P6
gate, on the reasoning that P6 rewrites the three games' `impl Route`
blocks anyway. **P6 rewrote them and the edge is still there**, and the
reasoning is what was wrong rather than the execution: rewriting an
`impl Route` block does not remove it. `front`'s six routes and all
three games' nodes still implement `ogham::route::Route`, so
`src/route/` still has consumers and ogham still declares
`structure = { path = "structure" }`.

Today: `cargo tree -p structure` is **one line**, absolutely, which is
§2's load-bearing half. `cargo tree -p contract --depth 1` is `ogham` +
`structure` and nothing else. `cargo tree -p ogham | grep -c structure`
is **3** — one real edge, plus two `synstructure` lines that are a
substring match inside the `skia-safe` subtree.

Closing it means the `Route` trait itself moving into `driver`, which
is a consumer-visible move in four repositories and is **not
scheduled**. It should be scheduled deliberately rather than deferred a
fourth time; deferring it three times is how it came to be true that
nobody had checked whether the reason would still apply.

### Phase 7, and where it stopped

Landed: `lorekeeper/docs/{MP_SEAM,CANVAS_COMPOSITION,REFACTOR_PLAN,
ROUTING}.md`, `regency/docs/history/player_client_outline.md`, the
`front` module docs, and the `AGENTS.md`/`CLAUDE.md` of ogham,
lorekeeper, regency and celia. untold_lore's `CROSSING.md` and
`ROOMS.md` took their amendments and three citations came off its lint
allowlist before the regression above was reported; the rest of that
repo's corpus is **held** until the regression is understood, because
what those documents say about the crossing and the instance tier may
depend on what it turns out to be.

**`APPLICATION.md`'s header is deliberately not flipped.** The last
phase gate cannot pass while a shipped game is black in the world view,
whatever the suite says. The header stays honest until it does.

**Also held:** untold_lore's nine-line `#[cfg(test)] mount_for_test`,
which now collapses to about five lines since `Assets::mount` is
public. It lives in `ul-client/src/client.rs`, which is where the
regression's fix is likely to land, so it is sequenced after.

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

### WP-0.1 Hot reload revalidates and re-projects — **landed**

> ogham `dc30634`, lorekeeper `b9d0dc5`. `Ogham::frame` now returns a
> `FrameReport { reloaded, rerendered }`; on a reload `Chrome` clears
> the stale error, calls `forget_projection()` (its first caller
> ever) and re-runs `validate` against the ids the mount remembered
> (`expected: Option<Vec<RouteId>>` — `None` when the host never
> validated, so a reload cannot invent a check the mount never asked
> for). Three tests in `tests/route.rs`, including a drift named
> without restart driven through the real file watcher. lorekeeper
> needed only a doc amendment: its one call site is `Chrome::frame`,
> whose signature did not change.
>
> Finding: the "unchanged keys hold config defaults" half of the bug
> was already mostly mitigated by `Ogham::carry_host_state_into`,
> which copies live host state into the replacement runtime.
> `forget_projection()` on reload is belt-and-braces; the genuinely
> absent fix was the re-validation.

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

### WP-1.1 Extract routing from the surface framework — **landed, acceptance amended (§0.5)**

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

### WP-1.2 The table grows tiers, areas, guards, documents — **landed** (`fa7092e`; API in §0.5)

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
test green against the re-export. — **passed 2026-08-20**, against
the baseline reds recorded in §0.5.

---

## Phase 2 — the store (repo: ogham, crate `structure`)

The order inside this phase matters; each WP builds on the last.

### WP-2.1 Schemas and derived reflection (§4.1, §4.3) — **landed**

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

### WP-2.2 The store core (§5.1-5.5, §5.7) — **landed**

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

### WP-2.3 The write side (§4.4) — **landed**

A scope publishes its accepted intents (typed, derived alongside
the schema); a document's raises validate at load against them;
a validated raise lands as an outbox action. This replaces the
`mpsc::Receiver<(String, Vec<RaiseArg>)>` seam
(`front/src/host.rs:64`) and the stringly decoding idiom
(`untold_lore/ul-client/src/chrome.rs:532-625` `intent_from_raise`
is the anti-corpus — its `text(0)? == "focus"` parsing and the
"RaiseArg has no as_bool" workaround at `chrome.rs:544-547` must
be unwritable in the replacement).

### WP-2.4 Validation, two grades (§4.1) — **landed**

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

> **Landed** as `ogham::contract::Documents` (`src/contract.rs`) over
> `structure`'s grading (`structure/src/validate.rs`) — the §2 edge
> again: grading has no I/O and stays in the dependency-free crate,
> while translation and file reading live where the parser is. A
> consumer instantiates it with `Mount { document, scopes
> (nearest-first), screens }` per shipped document; when P4/P5 land,
> the binding supplies all three from the table (`document_of`, the
> walk, the ids under the enclosing instance). It is a `cargo test`
> guarantee because `Documents::new` takes a `Store` with only its
> registrations run — no route walked, no scope mounted, no frame,
> no window, no Skia surface — and parses the shipped `.ogh` files
> without compiling bytecode.
>
> **Refusals:** `Unprovided`, `Shape`, `Unaccepted`, `Raise`,
> `Unpublished`. **Reports:** `Unread`, `Unraised`, `Undrawn`,
> `Unrouted`, `Shadowed`.
>
> **Appendix C coverage, per game.** untold_lore:
> `every_declared_key_is_projected` — forward half a refusal, reverse
> half *unrepresentable* (a producer can only set fields the schema
> declares, checked at startup), so it deletes;
> `every_declared_raise_reaches_a_handler` — forward half a refusal,
> reverse half now a **report**. regency: the `lobby.rs`
> schema-conformance test and `every_declared_raise_has_a_handler`
> delete; `the_shipped_document_declares_exactly_the_registered_screens`
> is covered as a **report**. celia: `every_declared_raise_has_a_handler`
> and the wire half of `every_button_reaches_a_route` delete;
> `the_shipped_document_declares_exactly_the_registered_screens` is a
> **report**. Not covered, staying: the path half of
> `every_button_reaches_a_route`, and
> `pause_does_not_survive_a_trip_to_the_menu`. Not in the ledger and
> correctly untouched: `every_registered_screen_renders` (celia and
> regency) and `the_documents_typefaces_are_registered`.
>
> **Also fixed in passing:** `reload_file` cleared the surviving
> tree's focus stack and portal layers *before* building the
> candidate runtime, so a hot edit that failed to compile took the
> running document's focus and portals with it. The clear now runs
> below the gate.

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
