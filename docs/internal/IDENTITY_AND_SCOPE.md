# Ogham — Identity & Scope (workshop draft)

> **Status: workshop draft. NOT a contract.** This captures a multi-session
> design reckoning about *what Ogham is for* and *what it should keep, cut, and
> defer*. It will be workshopped over several sessions and, once settled, will
> inform new tenets in [`INTENT.md`](INTENT.md) and the cut/keep sequencing.
> Until then, nothing here overrides existing contracts. Where this doc and the
> live tenets disagree, that disagreement is the *agenda*, not a directive.
>
> Decision legend used below:
> **[DECIDED]** — agreed this session, stable enough to build toward.
> **[LEANING]** — consensus direction, not yet pinned.
> **[OPEN]** — genuine fork, needs a future session.
>
> First drafted: 2026-06-07.

---

## 1. The reckoning

Ogham has been quietly trying to be **two frameworks in one binary**:

- **A "JavaScript + React replacement" / standalone app platform.** This
  identity funds: local `useState`-style state, the `OghamState`/`OghamMsg`
  typed-binding layer (the React+TS analogue), `mutation()` request/response,
  the standalone `client` viewer with `Ctrl+O`, portals/modals, and the LSP. It
  presupposes *Ogham is the application* and the host is a thin shell.

- **A "UI framework for Rust applications."** This identity is host-state-in /
  events-out: the host owns all state and logic, and Ogham is a *projection
  surface* over it. Here the Rust domain model is the source of truth and the
  `.ogh` renders + emits.

**The framework ships both and has committed to neither.** That indecision is
the root cause of most of the weak spots found in the audit: unused surface that
still has to be maintained, two rival typed-state systems, and a dual state
model that confuses authors.

### What the consumers actually voted for

Revealed preference across the three real consumers (small_mercies ~7.7k `.ogh`
LOC, untold_lore ~19k, lorekeeper 146 — a fossil) is decisive:

| Signal | Finding |
|---|---|
| Typed bindings (`derive(OghamState/OghamMsg)`) | **1 site total** (UL `chest_ui.rs`); 0 in SM, 0 in LK |
| `ogham check` / strict validation in any build/CI | **none** |
| Local `state` declarations | **1 in SM, ~10 in UL** (several serving dead drag/hover surface) |
| Lifecycle hooks (`on_mount`/`on_unmount`/`effect`/`cleanup`) | **0** in real `.ogh` (only the test suite) |
| Portals | **0 SM / 1 UL**; SM overlays are plain stacked flex + frosted glass; 0 Rust portal calls |
| `svg` widget / `mutation()` / drag (`draggable`/`on_drag`) | **0** anywhere |
| The schema-driven `editable` derive | **spreading across all authored content** up to the `Content` root; an active refactor |

Meanwhile the *render + language core* is heavily used: `for` (~21 SM / 29 UL
files), `match` (27 / 40), imports (27 / 41), spread-as-conditional-children
(~50 in SM), function-components, closures-as-event-handlers (112 `on_*` in SM),
flex-with-wrap (5 / 8), `text_input` (29+ in UL).

The verdict writes itself: **when the developers hit the exact need the typed
React-style layer was meant to serve — typed, introspectable host state — they
built a *second, better* system (`editable`) outside Ogham entirely**, because
Ogham's version was scoped to the wrong problem (view-binding validation) and
lived at the wrong layer (coupled to the renderer, so a domain type couldn't
derive it without dragging in the UI).

### Decision

**[DECIDED] Commit Ogham to the "UI framework for Rust applications" identity.
Shed the standalone-browser / JS-replacement ambition as a *deliberate
deferral*, not a thing to keep half-building toward.**

The dream of replacing the JS ecosystem is a legitimate *north star* but a
terrible *navigator*. The current browser surface delivers all the costs of the
ambition (maintained dead code, two state models, a rival typed layer) and none
of the thing (it is not three percent of a browser). The way to eventually
displace JS is the way JS won: dominate one embedded niche so completely it
becomes the obvious substrate, then grow outward under real demand. Premature
breadth is how challengers lose.

**The preserved option is the four-pass pipeline** (scanner → parser → compiler
→ VM). That spine already pays rent (it runs `for`/`match`/components) and keeps
the door open to richer in-language logic later *without a rewrite*. Portals,
`mutation`, the dynamism tail, `OghamState`, the standalone-app posture are
**not** option value — they are depreciating inventory that rots while it waits
(proof: the `compile_increment`-on-upvalue bug at `compiler.rs:1255-1257`, a
feature that is unused *and already wrong*).

> If the JS-killer is ever a real goal, it deserves its own roadmap and timeline
> — not a standing tax on the game projects. Conflated, both lose.

---

## 2. Keep / Cut / Defer

### Keep — the Rust-app-UI core

- The four-pass pipeline and the VM. Language features in real use: `for`,
  `match`, spread-as-conditional-children, function-components, imports,
  closures-as-event-handlers, `range` *inside* `for`, string `+`/index.
- Render engine: flex-with-wrap, reconcile-by-key (INTENT §3/§5),
  presence/animation, `text`, `text_input`, `image`, the Skia backend,
  hot-reload / file-watcher.
- `client` **as a dev previewer only** — a tool, not a platform claim.
- **Local state.** See §3 for the principle that keeps it from rotting into a
  back-door for host-defined state.
- **Grid** — [DECIDED] keep. It's a flex specialization (INTENT §4), nearly
  free, and UL uses it (5 files).
- **LSP + structural diagnostics** — [DECIDED] keep. Hover, goto-def, semantic
  tokens, scanner/parser diagnostics genuinely reduce friction for *agents
  hand-authoring `.ogh`*, which remains a substantial fraction of player-facing
  UI. Note it **re-sources, not dies**: it stops validating against `OghamState`
  manifests and validates `.ogh` field references against `editable` schema
  blobs instead (see §4).
- **`Surface` / `RenderContext` seam** — [DECIDED] keep, but **demote the
  rationale**. It is no longer "backend portability" (Skia is the only impl and
  GPU/tile-baking lives *outside* Ogham, so there is no second backend coming).
  It survives as a **paint-isolation + test seam**: it keeps `skia_safe` out of
  `widget/` (layout, hit-test, animation stay backend-pure and unit-testable)
  and prevents the skia-leak drift INTENT §6 warns about. Rewrite §6's *why*
  accordingly. (With `svg` cut, only `text_widget` still leaks skia, for text
  measurement.)

### Cut — safe immediately (zero consumer entanglement)

- The rival typed layer: `OghamState`/`OghamMsg`, `runtime/schema.rs`,
  `parser/typed_bindings.rs`, `ogham check`, and the typed half of
  `ogham-derive`. Superseded by `editable`; 1 site.
- Lifecycle/effects: `effect`/`cleanup`/`on_mount`/`on_unmount`,
  `RegisterEffect`/`EffectSlot`, the mount/unmount drain queues,
  `cancel_unmount_for_prefix`. 0 uses.
- `mutation()` request/response seam. 0 uses.
- `++`/`--` increment ops (unused, verbose desugar, latent upvalue bug).
- `svg` widget. 0 uses.
- Dead scaffolding: the `Environment` tree-walker leftover, `Placement`/`Gating`
  (inert), the `Dim` backdrop policy (TODO stub), `bezier_curve_to`/`close_path`.

> Note: `range` as a *standalone* expression is dead, but `range`-in-`for` is
> load-bearing — keep the parse path, it feeds for-loop bounds.

### Cut — decision-gated (cut, but a migration moves first)

- **Portals.** Effectively cut, but UL's single `.ogh` use must migrate to flex
  layering — which is already how SM does every overlay, so the target is proven.

### Defer — behind the seam, demand-gated (NOT built now)

- **Fine-grained reactivity engine** (subtree re-execution keyed on which struct
  changed). See §3, rung 2. The seam is cheap and we take it; the engine is the
  exact "build VM machinery for a scale we don't have" we're swearing off.
- **Scoped / declared component reads** (the isolation benefit). See §3 note on
  implication #3. A separate, bigger language decision, in tension with
  moddability.
- **The standalone-browser ambition** itself. Re-addable later via the preserved
  pipeline; not built toward in the interim.

---

## 3. The state model

**[DECIDED] All *host* state is schema-described (via `editable`); local state
stays free-form and ephemeral.** This sharpens the host/local boundary into two
clean, mechanically-distinct disciplines:

- **Host state** — uniformly schema-described, typed, introspectable,
  moddable-at-the-data-layer. *Always.* "Schema-described" means *any*
  `editable::Kind`, **not** rigid records: dynamic/data-driven shape (e.g.
  mod-defined resources) is `Kind::Map`; variant data is `Kind::Union`. Total
  *coverage*, not total *rigidity*.
- **Local state** — free, ephemeral, view-owned. *Never* schema'd.

### The dividing line (drift indicator)

The boundary is **who names the state into existence**:

- State the **host defines** (current character, resources, save-relevant
  selection) → host state. Putting *this* in a local cell is the bug — saves and
  deep-links silently lose it.
- State the **view invents** (a mod's custom tabs, hover, open/closed, an
  in-progress input buffer) → local state, *because the host literally cannot
  name it*.

This is the present-tense (not future-proofing) justification for keeping local
state: if a mod can restructure the UI — fold two modes into five custom tabs —
then "which tab is active" is state the host *cannot* own, because it never
defined those tabs. Local state is the mechanism that makes the decoupling
possible. The erosion to lint against is **host-defined semantic state leaking
into local cells**.

### Honest accounting of the gains from "all host state is a struct"

We adopted this *with eyes open* about what it actually buys now vs. later:

**Real, present-tense gains:**
1. **Dissolves the LSP/typo residual edge.** There are no unschematized host
   keys, so the "is this key typed or ad-hoc?" gradient — and the question of a
   featherweight key-manifest to typo-check ad-hoc keys — simply disappears.
2. **Uniformity.** One mental model for host state; no two-tier boundary to
   explain or police.
3. **A named change-unit seam** (rung 1, below).

**Deferred / speculative gains (do NOT build for these now):**
4. **Rerender granularity (rung 2).** Subtree reactivity keyed on struct change.
   *Valuable and dangerous* — flagged by the proposer as "not strictly
   necessary." Split it:
   - **Rung 1 — take now, nearly free:** a struct is a named change-unit. The
     host already does per-key change detection (`inject_host_state_if_changed`
     in SM `client.rs`); struct-level dirty-checking is a small step up and is
     present-tense useful.
   - **Rung 2 — defer:** widgets subscribing to a struct and skipping
     re-execution requires the VM to track *which subtree read which struct*
     (the signals/observable-store problem; path-based hook identity §9 is its
     prerequisite). Build only when a real app janks on global rerenders.
   Framing: struct-partitioned state is *preserved option value* for reactivity.
   Take the seam; defer the engine.
5. **Component isolation (implication #3) — real but NOT automatic.** Ogham's
   read model is *ambient* (plain identifiers fall through to the global host
   bag, §2), so structs-in-host-state isolate nothing by themselves. The benefit
   only materializes if components must *declare* which structs they see
   (props/inputs, not ambient global) — a separate, bigger decision that *trades
   against moddability* (ambient reads make a modder's "just show this here"
   easy; declared reads force threading). Keep it separable; don't bundle.
6. **Clearer typing (implication #4) — weakest.** The type system enforces
   *existence*, not *cohesion*; nothing prevents a junk-drawer `ScreenState`. A
   mild discipline nudge, not a structural guarantee. Don't lean on it.

### Consequence

The host now always emits *typed `editable` values* as its per-frame output
rather than a hand-built `Value` map. It largely does this internally already,
but it makes the `editable → Value` projection (§4) **load-bearing for all host
state**, not just editor panes. That projection is the long pole.

---

## 4. The `editable` ⟷ Ogham seam  [OPEN — crucial]

This is the most important unresolved design problem. `editable` (a **pure leaf**
— serde only, no Ogham/`app`/Skia) is the *single* schema/description system
(killing the `OghamState` rival). The question is how Ogham host state relates to
it.

### Not mutually exclusive: three coexisting things

(Correcting an earlier false dichotomy of "universal vs scoped".) "Universal"
means **single vocabulary, not universal coverage**: `editable` is the only
schema system, but these coexist permanently —

1. **`editable` content** — schema-driven; read/write in dev tools (Flow A),
   usually read-only in player UI (Flow B).
2. **Player-facing data** — now also schema-described per §3, but read-only into
   `.ogh` (host state flows in, never mutated by `.ogh`, §2).
3. **Pure stateless UI** — hand-authored markup binding to no data at all.

### Two flows, different halves of `editable`

- **Flow A — dev-tool UI** (`EditablePane<T: Editable>`): uses `schema()` *and*
  `apply()`. Ogham is a dumb renderer of a `view()` projection. (The generic
  `schema_form` inspector for this is *in progress* as of 2026-06-07 — SM
  `editable` step 5.)
- **Flow B — player-facing UI**: only ever *reads*, so it uses `schema()` + a
  read projection and **never** `apply()`. Edits leave as events; the host
  applies them via `editable::apply()` host-side. §2 preserved.

### The convergent design (three pieces)

1. **`editable::schema()` → JSON → the LSP/authoring manifest.** Replaces
   `ogham check`. The LSP validates `.ogh` field references against the schema
   blob. (This is the LSP "re-sourcing" from §2-Keep.)
2. **A derivable `editable → Value` read projection.** Replaces the 300+ hand
   `Value::String(...)` boxings at the injection boundary. The friction-killer.
3. **Edits flow out as events / `PaneAction`s, applied host-side via `apply()`.**
   No `SetHostState` opcode ever appears.

### The layering invariant (what makes it *right*)

**Ogham depends on `editable` for nothing.** It receives a `Value` tree to render
and, optionally, a schema JSON for the LSP. Both are produced *host-side*.
`editable` stays a pure UI-less leaf; Ogham stays domain-agnostic; **the host
composes the two.** That is the Rust-app-UI identity expressed structurally —
neither framework reaches into the other.

### The hard open sub-problem

**The `Value` ↔ `Kind` representation mapping.** Ogham speaks `Value`
(Int/String/Bool/Array/Map/Widget); `editable` speaks `Kind`
(Record/List/Union/Ref/Optional/Map/…). The projection concentrates on the hard
kinds: how does `Ref(table)` land — bare `String(id)`, or `{id,label}`? How does
`Union` project — `Value::Map{ kind: "...", <payload> }`?

**Payoff note:** the union-across-the-seam question is the *same problem* as the
`key == "__add"` / `"__objdel"` sentinel-key hacks (the ugliest wart in the
`.ogh` corpus, which exist precisely because there's no principled union
projection across the host boundary today). Solving the projection retires the
sentinels at the same time — it pays double.

---

## 5. Open questions for future sessions

- **[OPEN] The `Value` ↔ `Kind` projection** (§4) — the long pole; design it
  next. Unions and refs are the hard part; getting them right retires the
  sentinel-key hacks.
- **[OPEN] Scoped/declared reads vs. ambient reads** (§3 #5) — the component-
  isolation benefit collides with the moddability value. Separable from the
  state-model decision; decide on its own.
- **[OPEN] Cut sequencing** — order the safe-immediate cuts vs. the
  decision-gated ones (portal migration); confirm nothing in the "safe" list has
  a hidden consumer.
- **[LEANING] Local-state machinery slimming** — with lifecycle hooks cut, the
  path-based hook identity (§9) serves only `state`. Keep the cell; consider
  radically simplifying the identity machinery behind it.
- **Eventually:** translate the [DECIDED] items into `INTENT.md` tenets
  (identity/scope tenet; `Surface`-as-isolation rewrite of §6; the host/local
  state-discipline tenet). **Not yet** — that's the end of the workshop, not the
  middle.
