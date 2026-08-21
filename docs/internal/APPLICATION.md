# Ogham — The Application Model

> **Status: Design record — agreed direction, being built.**
> 2026-08-20. Phases 0 and 1 have landed: the structure framework
> exists as the `structure` crate (routing's table, walk and outbox,
> with §3.2's tiers and areas and §3.4's guard form as table data),
> and §2's dependency edge holds in the direction that matters —
> `structure` depends on nothing. The store (§5), the selection
> contract (§4) and the binding (§6) are not built. Build status,
> including the one amended acceptance criterion, is
> `APPLICATION_BUILD.md` §0.5. This header flips to "built" when the
> last phase gate passes.
>
> This document states what Ogham is becoming: **two composed
> frameworks and a contract between them** — a structure framework
> (routing fused with a scoped, schema'd state store) and a surface
> framework (the DSL, layout, and reactivity), joined by a thin
> binding. It was workshopped against untold_lore's 2026-08-20
> architecture audit and records the patched holes as axioms.
>
> When building starts, this document amends: `lorekeeper/docs/
> ROUTING.md` (the route tier leaves the engine's orbit and gains
> the instance tier and the store), `untold_lore/docs/CROSSING.md`
> axiom 4 / §6 (the route tier *does* learn areas; the presentation
> machinery survives) and §3 (the "world root needs no parking"
> rule is repealed: instance roots gain teardown/mount lifetimes,
> §5.6), and `untold_lore/docs/ROOMS.md` (a room's `Presentation`
> becomes a reading of its route node, and its `Requires`/`Refusal`
> columns move with it, §3.4). Those amendments are written in
> Phase 7, at the end of the build; until they are, those documents
> stand as written and this one is the direction. One of them is
> already half-true in the code: the route tier has left the surface
> framework's crate (WP-1.1), but nothing has been rewritten to say
> so yet.
>
> Amended 2026-08-20 after a four-repo consumer audit (untold_lore,
> regency, stargazer-celia, and lorekeeper's driver tier). The
> implementation plan is `APPLICATION_BUILD.md`.

---

## 1. Why: the history this resolves

Ogham began as a React replacement for Rust applications. The parts
of that thesis that hold up are load-bearing today: retained
reactive trees (the crossing's ghost works *because* an unprojected
tree keeps drawing), CSS-like layout, a hot-reloading DSL whose
moddability story is the WoW-addon model done with a real language.

What was never re-examined after the pivot away from "React
replacement" is the framework's **boundary**. Routing entered the
language in August 2026 (`src/route/`) on the argument that the
document already had `screen` blocks and scoped keys, so the
language should be able to decide a path. That argument was right
about the gap and wrong about the home, and untold_lore proved it
empirically within a week:

- **Most readers of the path are not widgets.** Which music plays,
  which backdrop stands, when a session boots or tears down, when
  input is gated, whether the body drives — all are functions of
  the path. The path is the *application's* central derived fact;
  the UI is one consumer among several.
- **The `host_state {}` block has the contract inverted.** The
  consumer's document declares what the provider supplies. No
  serious contract system works that way, and the failure mode is
  sharpest exactly where Ogham is strongest: a modder edits the
  declaration and manufactures the false impression that data
  should exist.

So: the view layer stays a view layer, and the things that crept in
— routing, and now state — become their own framework, *below* it.

## 2. The three pieces

One sentence each, per the single-responsibility rule:

- **The structure framework** (routing + store): what exists, what
  is true about it, and who is told when it changes.
- **The surface framework** (the DSL: documents, widgets, layout,
  reactivity, hot reload): how named surfaces look, given validated
  selections of state.
- **The binding / driver**: mounts instances, delivers frames,
  performs the crossing presentation. In a game this is the
  engine's driver tier (`lorekeeper/front` + `app`); a standalone
  application brings a minimal host trait implementation (§8.1,
  resolved).

The engineering guarantee is a dependency edge: **the structure
framework depends on nothing of the surface framework, and vice
versa; only the binding depends on both.** Cargo enforces it. The
surface framework's route-facing constructs (`screen`, `outlet()`,
selections) take *names* — strings and validated field paths —
never the structure framework's types. A single-document tool must
be able to use the surface framework with no router at all; if it
cannot, the split did not happen.

## 3. Axioms — structure

**3.1 Composed, not integrated.** Two frameworks with a seam, not
one framework with a router at its center. The decisive argument is
the mod surface: the route table and schemas are compiled Rust the
developer owns; documents are the hot-reloading consumer surface
modders touch. Integration would force that line to be drawn
*inside* one framework, where it will smear.
*Drift indicator:* a surface-framework crate importing a structure-
framework type; a route decision made by document code.

**3.2 One path space; a node is an instance root or a view.** The
application has a single route table (a DAG; a path is a walk,
derived per frame by `resolve_child` — no push/pop stack). Every
node declares its tier: an **instance root** mounts its own
document (menu, world, library — crossing one is teardown/mount
with a marked passage), a **view** selects a screen/outlet within
the enclosing instance (main menu → worlds list is two views in one
instance). Music, backdrops, session lifecycle and the UI read the
same path.
Three refinements from the consumer audit. A node may instead be
**structural** — mounting nothing, selecting nothing — existing to
own a scope whose lifetime spans its children (celia's `session`
node holds the focused character precisely because that fact must
outlive both the lobby and the arena). An instance root may carry
an **area** attribute — the ground its music and backdrops stand
on; untold_lore's `side_of` becomes this attribute, *not* the tier
itself (its four roots share three areas, and roster → world must
not read as a marked passage). And because the table is a DAG, one
node may have parents in several instances (settings under title
and under pause); the screen it selects resolves against the
*enclosing* instance's document.
*Drift indicator:* a host hand-driving a second document "exactly
as the router drives the first" (untold_lore's `project_root` was
the smell); an instance-boundary fact (`side_of`) living in game
code.

**3.3 The router derives; hosts apply.** A route's only way to
change the world is an action on the outbox, applied by the host's
services. Transitions that boot servers or tear down sessions are
host reactions to a path change, never route-tier behavior. (This
is the half of CROSSING.md axiom 4 that survives; the half that
falls is "the route tier knows nothing of areas.")
The converse also holds and is not a violation: some hosts react
to facts *upstream* of the path — a server-authoritative game
gates its simulation on the session's stage, from which the path
itself derives (celia's fight tick must keep reconciling under
`[arena, pause]`). The path is the central derived fact of
*presentation*; it is not always the source, and re-gating such a
host on the path would be wrong-way dataflow.

**3.4 A node guards its own door.** A node may declare
preconditions over store fields; a precondition that fails is a
machine-readable **refusal** — a sentence, written once and read
everywhere it surfaces (the panel row that grays, the toast that
explains). Ask-then-mount survives from ROOMS.md: entering is a
request the guard rules on, never a fait accompli. Without this,
every game keeps a shadow rooms table on its side of the seam.
*Drift indicator:* a `can_enter_x` boolean in a scope schema; a
consumer composing its own refusal sentence.

## 4. Axioms — the contract (provider-owned, GraphQL-shaped)

**4.1 The provider owns the schema; the consumer owns a
selection.** Each scope's schema is a Rust type declared with the
route node that provides it. A document declares what it *consumes*
— the `host_state {}` block survives with its meaning inverted,
from "here is what exists" to "here is my selection." Validation
runs at document load **and at every hot reload**: a selection
naming a field that does not exist is a load-time refusal that
names the field. A consumer edit can produce a loud, immediate,
named error — never a false expectation. Optionality is stated in
the schema ("this may be absent while X"), not by convention. So
are **defaults**: a field's at-mount value is declared, because a
silent zero-default can render as an invisible chrome
(untold_lore's `launch_fade` lesson). Validation has two grades: a
selection naming a missing field **refuses** (the modder's case —
loud, immediate, named), while table-coverage drift (a screen no
node reaches, a published intent no shipped document raises)
**reports** without refusing — the second is a developer mid-build,
and in a modding world a provider legitimately publishes intents
no shipped document uses. A hot-reload refusal rejects the new
document and names the field without tearing down the running
instance.

**4.2 Selection is per-field.** Per-field selection gives the most
specific refusal (the failure being designed against is a modder's
stale expectation), the finest invalidation, and per-field diffing.
Collections (`Vec<Row>`) are one field in v1 — a row change
invalidates the list's listeners. That coarseness is accepted and
named; the future answer is keyed collections (GraphQL-list /
React-key shaped), and nobody patches around it inside a consumer.
untold_lore hits this coarseness on day one (`entries`, the
wardrobe tree; regency's debug rows), so keyed collections are a
fast follow, not a someday.

**4.3 One schema, derived reflection.** Typed consumers (Rust) read
the schema struct directly at compile time; dynamic consumers
(documents) are validated against a *reflection* of it. The
reflection is **derived** (a derive macro — the move
`#[derive(Editable)]` already proved), never hand-maintained. This
is what makes the untold_lore triple-list (struct ↔ `.ogh` ↔
projection, guarded by `every_declared_key_is_projected`)
unrepresentable rather than merely guarded: the struct *is* the
schema, and the guard tests delete.

**4.4 Raises are the same contract, write-side.** A scope publishes
the intents it accepts exactly as it publishes the fields it
provides; a document's raises validate at the same load. The
two-way raise/handler guard test becomes structural and deletes.

**4.5 The mod surface is the consumer side, with a provider-side
dial.** Consumers cannot amend contracts, period. What schema a
consumer class *sees* is a provider decision (the SQL grants/views
idea): shipped documents and third-party mods need not see the same
schema, and the difference is declared, not an honor system. The
moment third-party documents exist, the schema is a public API and
its evolution policy must be stated explicitly — even if the
statement is "mods pin a game version; removals break loudly at
load." (Open, §8.)

**4.6 A selection binds top-level names.** A document's selection
binds the fields it selects to top-level identifiers, so a helper
reads `hud.clock` exactly as it did when the names were declared
locally. This is a migration property, deliberately: with it,
regency's document split touches zero helper bodies; without it,
the split is a three-thousand-line touch of every function in the
file. The binding syntax is part of the contract, not sugar.

**4.7 A shared module selects against more than one scope.** A
module mounted under two providers (untold_lore's sea panel lives
under the world root and under the editor) states its selection
once, as a **fragment**, and the fragment validates against each
providing scope at each mount. Validation is structural —
field-by-field against the derived reflection — never nominal,
because names cannot cross the Rust/document boundary; a record
whose field went missing refuses at load like any other selection
error, including the engine-published record shapes a game's
document consumes (§8.2).

## 5. Axioms — the store

**5.1 Two consumption verbs.** GraphQL has queries *and*
subscriptions; so does this protocol, from day one. **Subscribe**:
push, sparse, per-field notification, selective rerender — the
document idiom. **Read**: pull, per-frame, bulk, over the same
validated selection — the renderer idiom. A consumer declares which
it is. "Protocol" means *contract*, not serialization: an in-process
typed consumer pays direct field reads and a version check, not
dispatch through a map.

**5.2 The store/simulation fence.** The store carries **facts about
what should be presented** — the path, frame parameters (camera
config, lens, submersion, exposure, backdrop side, time of day),
projected view rows — and **never the presentation's bulk data**.
Meshes, bone buffers, terrain tiles, fifty thousand scattered rocks
flow through their own derivation disciplines; the renderer
consumes the store for parameters and the world for geometry. This
axiom carries "no lobby" force, because the failure mode — the
store becomes the god object, now with a protocol — is the 2026-08
audit's findings reincarnated one abstraction up. "Everything is a
consumer" is an attractor; this is the fence.
*Drift indicator:* per-frame bulk data (a vertex list, a tile, a
pose) appearing as a schema field; a consumer reading the store
inside a hot loop it previously read a snapshot in.

**5.3 Writes are actions; sets are equality-checked.** The store is
single-writer per field: producers (the host's services tier) set;
consumers read and subscribe; everything else goes through the
outbox. The store equality-checks every set and swallows no-ops —
which is lorekeeper INTENT §22 ("dirtiness is derived from values,
never remembered by flags") implemented once, centrally, replacing
every hand-rolled diff idiom (`drive_music`, `backdrop_showing`,
`drive_crossing` are all bespoke instances). The one exception
survives from the audio lesson: state living *outside* the process
(an audio bus, the OS cursor) is still diffed against a read-back
of the world, because the store can only be authoritative for what
it owns.

**5.4 Frame-transactional commit.** Writes accumulate during a tick
and commit at a frame barrier; notifications are batched and
delivered once per frame; consumers rerender after commit. No
consumer may observe field A new and field B old from the same
tick (the classic FRP glitch), and ten sets in one tick are one
rerender, not ten. This is an axiom of the protocol because it is
unrecoverable as a retrofit.
Within the tick the order is pinned: outbox drain, then producer
sets, then the barrier — an intent raised this tick lands in this
tick's commit, so a click's echo costs zero frames (celia's tile
focus is the regression test for this sentence). Producers run
inside the barrier and read working state; consumers only ever
see committed state.

**5.5 Facts, not presentation.** Eased values, glides, presences —
easing lives in consumers; the store carries the fact being eased
toward. (The existing tick/draw and client-local-presentation
disciplines, inherited unamended.) And producers set facts at the
granularity consumers act on: the sky clock floors to the minute,
a countdown to the second — a raw per-frame float defeats the
equality check and wakes every subscriber every frame.
Quantization is the producer's duty, and a schema may declare a
field's grain.

**5.6 A frozen consumer is the ghost.** A departing instance is an
unsubscribed-but-retained consumer: it keeps drawing its
last-delivered values until the binding drops it. The crossing's
freeze rules (CROSSING.md §3) stop being special machinery and
become what unsubscription *is*. Two riders from the audit: a
ghosted instance retains its draw slot (§6.2) until dropped — the
world must keep painting under the departing document; and an
instance whose backdrop is host-painted ghosts as a **captured
composite frame**, because its painter cannot re-run against
torn-down simulation state (celia's arena; `Departure::Bleed`'s
existing semantics, promoted to the instance tier).

**5.7 Derivation is a producer.** Derived facts — view rows, joins
over the snapshot, an item card assembled from three panes — are
ordinary producers subscribed to the fields they derive from,
running inside the same tick's barrier (§5.4), so no consumer ever
sees a torn derivation. This is where untold_lore's `reproject`
joins go: they do not dissolve, they relocate and gain a trigger.
If producer-on-producer ordering ever grows hair, the answer is a
declared derived field (the signals family), never ad-hoc
sequencing inside a consumer.

## 6. Axioms — the binding

The §2 sentence — mounts instances, delivers frames, performs the
crossing — unpacked, because the consumer audit found every one of
these being hand-rolled somewhere:

**6.1 The binding mounts what the table names.** An instance root
names its document; the binding mounts, projects, and tears down.
No host hand-mounts a second document (`mounted_ui`, `own_ui`,
`brings_own_document` delete — untold_lore's workspaces held as
`Option<EditorClient>` fields were the drift indicator realized).

**6.2 One draw slot per instance root.** What paints *under* an
instance's document is declared at the mount: an optional backdrop
painter fed by store frame parameters (read verb) and world data —
its signature does not pass the store's write surface, which is
§5.2 drawn as an API instead of a discipline. Whether the instance
is opaque or composites over its painter is mount data too
(retiring per-route `occludes()`), and a painter that animates
continuously says so, because a widget tree only repaints when
dirtied.

**6.3 Side channels are named and validated.** Anchors (a host
point positioning document content) and painter payloads
(per-frame paint positioned by document layout) are binding-owned,
per-instance channels — per-frame geometry that must *not* be
store fields (§5.2) but must not be stringly either. Their names
validate at document load exactly as selections do; a typo'd
anchor is a refusal, not a silently empty portal.

**6.4 The crossing belongs to the binding.** The binding detects
an instance-root boundary in the path delta itself (no game diffs
`side_of` by hand), mounts the arriving instance *before* the
sweep begins, runs a parameterized default passage (direction,
duration, easing), and lets a game override per edge (`Departure`
— regency's page bleed). Presses are swallowed during a crossing;
releases flow. The passage's side effects — whoosh, music,
backdrop — are host reactions to the path change (§3.3), never
crossing code.

**6.5 Caches are shared across instances.** Images, fonts, and
their decode products live at the binding, not per instance — a
crossing must never re-decode the arriving document's art on the
crossing frame.

## 7. What this dissolves in the consumers

Stated so the payoff is checkable, not vibes:

- untold_lore's `chrome::State` (78 projected keys over 77 fields,
  one namespace) becomes per-scope schemas with route-node
  lifetimes; on the main menu the gameplay scope does not exist,
  and vice versa.
- `project` (535 lines of mechanical transcription) deletes into
  derived reflection; `reproject` (313 lines/frame) splits — its
  per-frame diff and its scope-arm skeleton are the store's, its
  joins relocate into producers (§5.7).
- `every_declared_key_is_projected` and
  `every_declared_raise_reaches_a_handler` delete — their
  properties become unrepresentable.
- `UlGame::project_root` deletes — the router mounts and projects
  every instance, menu and world alike.
- `crossing::side_of` moves into the route table as the instance-
  root tier of the nodes.
- The rooms table's `Presentation` becomes a reading of the route
  node; "room" and "stance" retire as vocabulary (a room is a
  routed view; a stance is an overlay — `Overlay { keys_reach_body
  }` vs `Page` collapses the enum, `Stance` folding in as
  `keys_reach_body: false`; the table's other columns follow §3.4).
- The ul-editor family joins the ledger: `EditorShell::host_state`
  (176 lines), `map_chrome.rs` (647 lines), and the per-frame
  reinjection loops are a second hand-rolled driver whose
  documents declare no contract at all; its three `Page` instances
  become instance roots and its injections become schemas.

The same audit's ledgers for the other consumers:

- regency: the ~254-line record block and `host_state` delete; the
  schema-conformance and raise/screen guard tests delete; sixteen
  hand-rolled equality guards and the `panels_dirty` idiom die
  into the store; half of `teardown`'s manual remembering becomes
  unrepresentable (scope state dies with its node); Wheel and
  Epilogue promote from store facts to views, making stage ↔ path
  a bijection and deleting the page-turn edge detector; the page
  bleed becomes the crossing's marked passage, fed by the ghost —
  which also fixes the inbound capture that bled the backdrop but
  popped the invitation card.
- celia: `HostCx::project_root` deletes outright (the root
  headings become per-document literals); the raise mpsc plumbing
  and ~150 lines of guard tests delete; the pause-claim
  choreography, duplicated across both routes with its own
  regression test, dies into node-lifetime scope state; the
  lobby → arena crossing gains the deployment ceremony the old
  machinery never allowed.
- lorekeeper: the `shell` crate deletes (ROUTING.md phase 8b, at
  last); `Chrome`'s per-key diff map *is* the proto-store and
  moves down into it; `drive_menu_music`'s memo bool becomes a
  path subscription.

## 8. Open questions

1. **The standalone driver.** Resolved by the audit: yes, and it
   already exists in embryo. The minimal host trait is `HostCx`
   minus its game-flavored methods — render context ready, window
   controls, tick producers, gesture stream — with the engine's
   `app` as one implementation and `ogham_preview` (which already
   hand-drives a single instance) as the free second one that
   proves the trait. What it must not include: anything naming a
   session, a save, or a capability — those are a game
   framework's opinions.
2. **Engine stock screens.** Resolved: the engine publishes, per
   stock screen, the scope schema (a Rust type whose derived
   reflection documents validate against), the intent vocabulary,
   and the producer. Title, connect, settings, and the save
   browser ship as an engine **front-of-house instance root**
   games compose into their tables; pause, settings, and saveload
   additionally splice as views under game-owned roots (pause can
   never be a root — the world must keep drawing behind it). The
   stock document ships and is restyled per `screen` block — or
   replaced wholesale, the engine contribution that survives
   replacement being the schemas, intents, and producers;
   surfaces stay the game's, which is what lets celia's paperwork
   dress and regency's engraving survive.
3. **Mod-facing stability policy** (§4.5): stated the day
   third-party documents exist, not after.
4. **Naming.** The split is the moment to retire the "ogham"
   name (pronunciation). Criteria fixed now: name the pair as a
   family; unambiguous English pronunciation on sight; one–two
   syllables; crates.io/GitHub availability; no collision with
   established Rust projects. The pair should read as *structure*
   vs *surface*. Candidate generation waits until the seam is
   settled, then runs with an availability check.

## 9. Precedents consulted

GraphQL (provider schema / consumer selection / introspection /
subscriptions-vs-queries), the wasm component model's WIT (host
publishes, guest imports, instantiation-time refusal — the modern
answer to the modding contract), protobuf (explicit evolution
rules; contract as compiled artifact), SQL grants/views (per-
consumer-class schema projection), the signals family and re-frame
(per-field invalidation; subscriptions as derived signals), Elm
(single store, actions as the only writes) — and React context as
the anti-example, being exactly what a stringly, contract-free
`host_state` map is. The WoW addon ecosystem is the cautionary
tale on the modding side: a huge consumer surface with no static
contract, where every patch breaks addons at runtime, mysteriously.
