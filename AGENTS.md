# AGENTS.md

## Project Overview

Ogham is a UI language and framework for Rust applications. It provides:

- A custom language (`.ogh` files) for declarative UI development.
- A scanner and recursive-descent parser that produce an AST.
- A bytecode compiler and stack-based VM runtime.
- A Flexbox-based widget tree with reconciliation (React-style diffing).
- A Skia rendering backend, with a `Surface` trait for custom backends.

Ogham operates in two modes:

1. **Library** -- embed Ogham UIs inside a Rust application (the primary use case).
2. **Standalone browser** -- the `client` binary opens `.ogh` files directly.

## Architecture

```
Source (.ogh)
  -> Scanner (src/scanner/)      -- lexical analysis, produces tokens
  -> Parser  (src/parser/)       -- recursive-descent, produces AST
  -> Compiler (src/runtime/compiler.rs) -- compiles AST to bytecode
  -> VM       (src/runtime/vm.rs)       -- stack-based execution
  -> Widget values                      -- runtime widget representations
  -> builder (src/widget/builder.rs)     -- converts to widget tree nodes
  -> UI (src/widget/mod.rs)             -- layout, reconciliation, events
  -> Surface (src/skia.rs or custom)    -- rendering
```

### Key modules

| Path | Purpose |
|------|---------|
| `src/lib.rs` | Top-level `Ogham` struct; owns runtime, UI tree, and file watcher |
| `src/scanner/` | Lexer -- converts source text to tokens |
| `src/parser/` | Parser -- builds AST from tokens |
| `src/runtime/mod.rs` | `Runtime` -- execution state, host state, imports, rerenders |
| `src/runtime/compiler.rs` | Bytecode compiler |
| `src/runtime/vm.rs` | Stack-based virtual machine |
| `src/runtime/value.rs` | `Value` enum (all dynamic types) |
| `src/runtime/config.rs` | `RuntimeConfig` builder |
| `src/runtime/environment.rs` | Variable scoping |
| `src/runtime/error.rs` | `RuntimeError`, `VMError` |
| `src/runtime/descriptor.rs` | `WidgetDescriptor` -- runtime widget representation |
| `src/widget/mod.rs` | `UI` struct, `Surface` trait, `Widget` trait, `RenderEffects`, `TickResult` |
| `src/widget/builder.rs` | Converts runtime widget values to widget tree nodes |
| `src/widget/flex_widget.rs` | Flexbox container; owns style transitions and exit lifecycle |
| `src/widget/text_widget.rs` | Text display widget |
| `src/widget/text_input_widget.rs` | Text input widget |
| `src/widget/svg_widget.rs` | SVG widget |
| `src/widget/grid_widget.rs` | Grid container widget |
| `src/widget/image_widget.rs` | Image widget |
| `src/widget/canvas_widget.rs` | Host-painted `Canvas` leaf — `Painter`, `CanvasPainter`, the widget |
| `src/widget/presence_widget.rs` | Lifecycle-sequencing container (waits for exits before mounting next generation) |
| `src/widget/portal_widget.rs` | Portal — paints children into a per-frame layer with hit-test priority |
| `src/widget/portal_layer.rs` | Named portal layers + per-layer backdrop / cursor policies |
| `src/widget/animation.rs` | Spring math + per-property animation state used by transitions |
| `src/widget/event.rs` | Event types (`Event`, `EventContext`, `TickContext`, `DragState`) |
| `src/skia.rs` | Skia rendering backend (implements `Surface`) |
| `src/typed.rs` | `TypedOgham` handle for `#[derive(OghamState)]` / `#[derive(OghamMsg)]` integration |
| `src/diagnostics/` | Schema-diagnostic engine used by `ogham check` and the LSP |
| `src/cli/` | `ogham` CLI binary (currently `check`; `render.rs` is the diagnostic-formatting helper, not a subcommand) |
| `src/lsp/` | `ogham-lsp` language server binary |
| `src/file_watcher.rs` | File watching for hot-reload |
| `crates/ogham-derive/` | `#[derive(OghamState)]` / `#[derive(OghamMsg)]` proc macros |

## LSP Server

The `ogham-lsp` binary implements the Language Server Protocol for `.ogh` files,
enabling editor support and AI-assisted development.

### Building and running

```sh
cargo build --bin ogham-lsp        # build
./target/debug/ogham-lsp           # run (stdin/stdout transport)
```

### VSCode integration

The extension lives in `editors/vscode/`. Set `ogham.lspPath` in VSCode settings
to the path of the built binary (defaults to `"ogham-lsp"` on PATH).

### Capabilities

| Feature | Status | Notes |
|---------|--------|-------|
| Diagnostics | ✓ | Scanner + parser errors on every change |
| Hover | ✓ | Variables, parameters, widgets, keywords |
| Semantic tokens | ✓ | Full-document token classification |
| Go to Definition | ✓ | Jumps to declaration site of any variable or parameter |
| Document Symbols | ✓ | Outline of all declarations (useful for file navigation) |
| Completion | — | Not implemented |
| Find References | — | Not implemented |

### Using the LSP as an agent

When working on `.ogh` files, prefer LSP over grep for:
- **Go to Definition** — resolves scope correctly; grep does not handle shadowing
- **Document Symbols** — get file outline without reading every line
- **Diagnostics** — verify edits introduced no syntax errors

---

## Coding Conventions

- **Rust 2021 edition.**
- **`rustfmt.toml`**: `max_width = 100`.
- **Doc comments** on public items using `///` and module-level `//!`.
- **Builder pattern** for `RuntimeConfig` (`.with_host_state()`, `.with_event_handler()`, `.with_project_root()`).
- **Thread safety**: `Runtime` is shared as `Arc<Mutex<Runtime>>`.
- **Error handling**: return `Result` with `RuntimeError` or `VMError`; use `.expect()` only for lock poisoning.
- **Testing**: unit tests inline in modules (`#[cfg(test)]`), integration tests in `tests/`.

## Value Types

The `Value` enum (`src/runtime/value.rs`) represents all dynamic types:

| Variant | Rust type | Notes |
|---------|-----------|-------|
| `Integer(i32)` | `i32` | |
| `Float(f64)` | `f64` | |
| `Boolean(bool)` | `bool` | |
| `String(String)` | `String` | |
| `Map(HashMap<String, Value>)` | `HashMap` | Object-like key-value maps |
| `Array(Vec<Value>)` | `Vec` | Ordered collections |
| `BytecodeClosure(Rc<VMClosure>)` | | Compiled closure |
| `Widget(WidgetDescriptor)` | | A widget produced during execution |
| `WidgetRef(u64)` | `u64` | Opaque widget identity handle (Phase 2.5+) |
| `Mutation(Rc<RefCell<MutationState>>)` | | Reactive `state` cell handle |
| `BoundTrigger(Rc<RefCell<MutationState>>)` | | Reactive trigger bound from Rust via `TypedOgham` |
| `Void` | | Unit / no value |

When injecting state from Rust, build values using these constructors directly, e.g. `Value::String("hello".to_string())`, `Value::Integer(42)`, `Value::Array(vec![...])`, `Value::Map(map)`. For typed host state, prefer `#[derive(OghamState)]` + `TypedOgham` (see *Typed bindings* below) — it generates the conversion plus a compile-time schema the `ogham check` CLI and LSP can validate `.ogh` files against.

## Ogham Language Quick Reference

### Variables and state

```ogh
let x = 5;
let name = "Ogham";
state count = 0;          // persists across rerenders
```

### Functions

```ogh
let greet = fn (name: string): string {
  "Hello, " + name
};

// Implicit return (omit trailing semicolon, Rust-style)
let add = fn (a: int, b: int): int {
  a + b
};
```

### Widgets

Built-in widget types: `Flex`, `Text`, `TextInput`, `Svg`, `Image`, `Grid`, `Presence`, `Portal`, `Canvas`. `Flex` is the workhorse — it owns the layout/style/animation/lifecycle machinery; the other containers (`Grid`, `Presence`, `Portal`) wrap or specialize it.

Sizing: `width`/`height` take `"grow"`, `"shrink"`, a number (fixed px), or a percent. One pinned interaction (`flex_widget.rs` tests): while a `shrink` parent measures itself along an axis, `grow` descendants on that axis contribute their *content* size — a shrink parent has no leftover space to grow into — and are stretched to the parent's resolved size during the real layout pass. So a full-height accent bar (`height: "grow"`, no children) inside a `height: "shrink"` row contributes nothing to the row's height and then spans it exactly.

```ogh
Flex {
  style: {
    width: "grow",
    height: "grow",
    direction: "column",
    main_alignment: "center",
    cross_alignment: "center",
    padding: 24,
    gap: 12,
    background_color: { r: 30, g: 30, b: 30, a: 255 },
  },
  children: [
    Text {
      text: "Hello, world!",
      style: { size: 18, color: { r: 255, g: 255, b: 255, a: 255 } },
    }
  ],
}
```

### Events

Dispatch events to the host Rust application:

```ogh
Flex {
  mouse_down: fn () {
    event("tool_selected", tool_name);
  },
  children: [ ... ],
}
```

`Flex` and `Grid` accept four pointer listeners: `mouse_down`,
`mouse_up`, `mouse_enter`, `mouse_leave`. `mouse_enter` and
`mouse_leave` fire on the hover-in / hover-out transition during
`mouse_move` dispatch — every widget in the path from root to the
deepest hit fires `mouse_enter` when first entered and `mouse_leave`
when no longer hovered. `mouse_up` fires on the same hit-test path
as `mouse_down`. `TextInput` exposes `mouse_down` and `mouse_up`;
`Image` exposes `mouse_down`.

#### Drag events (Phase 3)

For real drag-and-drop, prefer the dedicated drag listeners over
hand-rolling on `mouse_down` / `mouse_enter` / `mouse_leave`. They
share a payload through `EventContext` and let any widget declare
itself a drop target via an `accepts_drop` predicate.

```ogh
Flex {
  drag_payload: { kind: "item", id: 7 },     // marks this widget a drag source
  drag_dead_zone: 6,                          // optional; defaults to 4px
  drag_preview: Flex {                        // optional; renders attached to cursor
    style: { width: 64, height: 64, background_color: cursor_swatch },
  },
  drag_start: fn (payload) { log "started"; },
  drag_move:  fn (payload) {  /* fires on widget under cursor */ },
  drag_end:   fn (payload) {  /* fires on drop target or originator */ },
}

Flex {
  accepts_drop: fn (payload: bool): bool { payload },
  drag_end:     fn (payload) { /* drop happened here */ },
}
```

The host Rust pump (which owns the dead-zone state machine) drives
dispatch through the runtime API:

```rust
let mut state = ogham.dispatch_drag_start(origin_widget_ref, payload, point);
ogham.dispatch_drag_move(&mut state, point);   // each subsequent mouse_move
ogham.dispatch_drag_end(&mut state, point);    // on mouse_up; clears the preview
```

`dispatch_drag_end` walks open portal layers high-priority-to-low
then the base tree, picking the deepest widget whose
`accepts_drop(payload)` returns `true`. If none accept, `drag_end`
fires on the originator (cancel-style behavior). The drag preview
widget renders into the `CursorAttached` portal layer at the
cursor; give it explicit `width` / `height` for a sensible result.

#### `contextmenu` event

`contextmenu` is dispatched by the host on right-click via
`Ogham::dispatch_contextmenu(point)`. It fires on the deepest
widget at the cursor (no automatic bubble — wrap if needed).
Use it instead of overloading `mouse_down` so you can route left
and right clicks differently.

```ogh
Flex {
  mouse_down:  fn () { event("select", item_id); },
  contextmenu: fn () { event("show_menu", item_id); },
}
```

#### Scroll and keyboard events

Scrolling on a `Flex` whose `style.overflow == "scroll"` is
handled by the widget itself (no listener needed). Keyboard
events flow to the focused widget (`TextInput` is the canonical
focus target — see *Cursor and key signals* in the integration
guide).

### Animations and lifecycle

Style transitions, entry animations, and exit animations are first-class on `Flex`. Three pieces work together: a `transition:` declaration in the style enabling spring interpolation, optional `initial:` / `exit:` widget-level styles for entry/exit animations, and a `key:` for stable identity across reconciles.

```ogh
Flex {
  key: "save-button",
  initial: { opacity: 0, transform: { translate_y: -12 } },
  exit:    { opacity: 0, transform: { translate_y:  -8 } },
  style: {
    background_color: { r: 60, g: 90, b: 120, a: 255 },
    border: { width: 1, color: { r: 90, g: 120, b: 150, a: 255 }, style: "solid" },
    padding: 12,
    transition: {
      opacity:          "spring",
      transform:        "spring",
      background_color: "spring",
      border:           "spring",
    },
  },
  hover_style: {
    background_color: { r: 90, g: 120, b: 150, a: 255 },
    border: { width: 1, color: { r: 140, g: 170, b: 200, a: 255 }, style: "solid" },
  },
  children: [ Text { text: "Save" } ],
}
```

**`transition`** — declares which style properties spring-animate when their target value changes (hover/unhover, state-driven style updates). `"spring"` uses defaults; `{ stiffness: 200, damping: 28 }` tunes per property. Animatable properties: `background_color`, `text_color`, `border`, `corner_radius`, `padding`, `margin`, `gap`, `text_size`, `opacity`, `transform`. Non-animated properties snap immediately.

**`initial`** — style snapshot the widget is born at. The widget's first frame renders at `initial`; subsequent ticks spring toward the declared `style` (target). Without `initial`, the widget mounts at its declared style with no entry animation.

**`exit`** — style snapshot the widget animates toward when it disappears from the declarative tree. The widget stays in the tree (a "ghost") until its springs settle, then is dropped. Requires a `key` so reconciliation can tell removal from reordering. Without `exit`, the widget is dropped immediately.

**`key`** — stable identity used by reconciliation to match widgets across frames. Required for exit animations and for preserving animation/hover/scroll state across reorders. Keyless siblings are matched by position.

#### `Presence` — transitions between generations of content

Wrap a swap point (e.g., page transitions) in `Presence` to give the outgoing content an exit animation when `key` changes:

```ogh
Presence {
  key: current_route_id,
  mode: "wait",   // optional; default "pop"
  children: [ render_route(current_route_id) ],
}
```

When `key` changes, every current child is asked to `begin_exit` (cascading through descendants if no own `exit` exists). What happens next depends on `mode`:

- **`pop`** (default): exiting children are popped out of layout flow — pinned as ghosts at their last layout rect, painted *above* the live children, invisible to input — and the new content mounts immediately, playing its entry animations under the fading ghosts. The transition costs `max(exit, enter)` instead of `exit + enter`. Rapid key changes accumulate self-draining ghost cohorts; reverting the key mounts a fresh subtree (the old generation's state was flushed at replacement — a revert is a re-entry, not a resurrection).
- **`wait`**: the old serial machine — exiting children stay in the tree as in-flow ghosts, the new content is held aside as *pending* and mounts only once all ghosts settle. Rapid key changes replace pending latest-wins; reverting the key mid-exit cancels the transition and unwinds the in-flight exits. Opt in when the entrance must not start until the exit has finished.

Use `Presence` for route boundaries; nest one per "slot" that should transition independently (e.g., separate Presences for a sidebar and a main panel). Details: `docs/internal/PRESENCE_POP.md`.

#### Opacity and transform style properties

`opacity` (number 0..1, default 1) and `transform` (`{ translate_x, translate_y, scale, scale_x, scale_y, rotate }`, default identity) are paint-only — they don't affect layout or hit-testing. `transform` pivots around the widget's center. Both are spring-animatable when listed in `transition:`.

#### `backdrop_filter` — frosted glass over what's behind a panel

`backdrop_filter: { blur: N }` (default `None`) blurs the canvas content already painted under the widget's border box. The widget's own background image / colour and any descendants composite *on top of* the blurred capture; the border draws sharp on the main canvas. `N` is Gaussian sigma in unscaled px — values of 8–16 read as "frosted glass" over typical UI imagery; values above ~32 collapse to a near-uniform tint. Backends that don't support backdrop sampling fall back to no-op via the trait defaults; the panel still renders, just without the frost.

```ogh
Flex {
  style: {
    background_color: { r: 22, g: 24, b: 28, a: 80 },
    border: { width: 1, color: hairline, style: "solid" },
    corner_radius: 8,
    backdrop_filter: { blur: 14 },
  },
  children: [ /* ... */ ],
}
```

**Cost.** `backdrop_filter` triggers a Skia `save_layer` with an `image_filter::blur` backdrop on every paint. Static panels at hub speed (~60fps with little behind them moving) cost a couple of ms; the same primitive on hot-path HUD elements painted over a moving 3D scene is much more expensive because the blurred capture can't be cached frame-to-frame. Use it for chrome that asks the player to slow down — not for combat-time HUD widgets. Sigma is *not* spring-animatable today; assigning a different `blur` value snaps.

### Lifecycle hooks (Phase 2)

Inside any function body, four block-statement keywords let you
attach side-effects to a widget's mount / unmount / dependency
changes. Identity is path-based — every `(call_stack, hook_id)`
pair is a fresh slot, so hooks survive reorder and re-render
without re-firing unless their dependencies actually change.

```ogh
let panel = fn () {
  state count = 0;

  on_mount   { event("panel_opened", count); };
  on_unmount { event("panel_closed", count); };

  effect (count) {                   // re-fires when `count` changes
    event("count_changed", count);
    cleanup { event("count_cleanup", count); };  // runs before next fire
  };

  Flex { children: [ /* ... */ ] }
};
```

| Hook | Fires | Re-fires |
|---|---|---|
| `on_mount { ... }` | After the function's first render in this path. | Never. |
| `on_unmount { ... }` | When the function's path *drains* — either dropped immediately on reconcile or after the owning widget's exit animation settles. | Never. |
| `effect (deps) { ... }` | After every render where any `deps` value differs from the prior render. | Each dep change. |
| `cleanup { ... }` | Inside an `effect` body only. Runs before the effect re-fires AND when its owning path drains. | Each effect re-fire / drain. |

**Drain timing.** Phase 3 ships true drain-time semantics: a
widget with an `exit:` style won't fire `on_unmount` until its
exit animation settles AND `drain_exited_children` removes it
from the tree. Cancellation (key reappears mid-exit) clears the
pending unmount.

**Mount timing.** Mounts fire inside `Runtime::rerender` —
specifically `pre_layout_drain`, after module execution and
before the host's layout pass. Mount bodies therefore *cannot*
read layout sizes from the just-rendered tree (the M1
implementation deferred post-layout mount timing; refining it
is on the backlog when Portal positioning needs it).

**Conditional hooks** (e.g. `if (x) { on_mount {...} }`) are
legal but the LSP warns about them — you almost certainly want
to gate the *value* a hook computes rather than whether the
hook itself registers, since registration determines path
identity. Restructure to put the condition inside the body.

### Typed bindings (Phase 1)

Two `#[derive(...)]` macros generate the bridge code between Rust
host types and the Ogham runtime, plus a compile-time schema
manifest the `ogham check` CLI validates `.ogh` files against
(LSP integration of the schema-diagnostic engine in
`src/diagnostics/` is on the roadmap but not yet wired):

```rust
use ogham_derive::{OghamState, OghamMsg};

#[derive(OghamState)]
#[ogham_state(binding_module = "settings")]    // exposes module name to ogham check
struct SettingsState {
    volume: f32,                // -> Float
    label:  String,             // -> String
    items:  Vec<String>,        // -> Array<String>
}

#[derive(OghamMsg)]
enum SettingsMsg {
    SetVolume(f32),             // .ogh: event("set_volume", v)
    Reset,                      // .ogh: event("reset")
}
```

Mount via the `TypedOgham` handle (re-exported from the crate
root):

```rust
use ogham::TypedOgham;

let handle: TypedOgham<SettingsState, SettingsMsg> =
    TypedOgham::watch("./data/settings.ogh", SettingsState::default(), config)?;

// set_state takes a full S; the wrapper diffs against the
// previous frame and only injects changed fields.
let mut next = current.clone();
next.volume = 0.8;
handle.set_state(next);

while let Some(msg) = handle.poll_msg() {       // typed event recv
    match msg { SettingsMsg::SetVolume(v) => /* ... */, .. }
}
// Or drain all queued messages in one go:
// for msg in handle.drain_msgs() { ... }
```

In the `.ogh` file, top-level identifiers `volume`, `label`,
`items` resolve to the host state directly; events fire through
`event("set_volume", value)`. `ogham check` cross-references
the manifest at analysis time, so a typo in
`event("setvolume", ...)` flags as an error before run. (LSP
integration of the schema-diagnostic engine is on the
roadmap; today the LSP reports scanner / parser / typed-
bindings AST validation / lifecycle conditional-registration
diagnostics only.)

### Portal widget (Phase 2 + 2.5)

`Portal { open, focus_trap, layer, cursor, anchor, children }` lifts its
children's paint and hit-test out of the parent's clip / order
into a named per-frame **layer**. Layout-wise the Portal node
contributes nothing to the parent's flow — children paint into
the viewport in a second pass.

```ogh
Portal {
  open: show_modal,
  focus_trap: true,                   // input is gated to this subtree while open
  layer: "overlay-modal",             // see layer table below
  children: [
    Flex {
      style: { width: 480, height: 320, /* ... */ },
      children: [ /* dialog content */ ],
    }
  ],
}
```

| Layer | Priority | Default backdrop | Default cursor | Use for |
|---|---:|---|---|---|
| `"main"` | 0 | None | Inherit | Reserved (base tree). |
| `"overlay-modal"` | 100 | Block | Free | Modal dialogs, escape menu. |
| `"popover"` | 200 | None | Free | Dropdown menus, comboboxes. |
| `"tooltip"` | 300 | None | Inherit | Hover tooltips. |
| `"toast"` | 400 | None | Inherit | Transient notifications. |
| `"cursor-attached"` | 500 | None | Inherit | Drag previews; positioned at cursor. |

**Backdrop policy.** A `Block`-policy layer with any open entry
both paints a translucent runtime backdrop AND suppresses click
fall-through to lower layers / the base tree. Authors can layer
their own styled backdrop on top of (or replacing) the runtime
backdrop.

**Focus trap.** When `focus_trap: true`, focus moves outside
the portal's subtree are rejected; `Ogham::has_input_blocking_portal()`
returns true while any open trapping portal exists.

**Cursor preference.** `cursor: "free"` declares the layer
wants a visible system cursor; `cursor: "inherit"` lets the
host decide. `Ogham::wants_cursor_free()` aggregates across
open portals + the focused widget.

**Composition.** Backdrop styling, dismiss-on-outside-click,
and Escape-to-dismiss are *not* Portal properties — they're
consumer composition with regular widgets. See
`examples/portals/components.ogh` for `Modal`, `Tooltip`, and
`Dropdown` reference functions.

### Anchored portals

A Portal that names an `anchor:` takes its viewport origin from a
point **your host sets**, instead of from the slot it was declared
in. It's the seam for chrome that has to follow something the host
knows about and Ogham doesn't: the pointer, an entity's projected
screen position, the field a popover belongs to.

```ogh
Portal {
  layer: "tooltip",
  open: tooltip_open,
  anchor: "action-tooltip",         // names a host-set anchor
  anchor_policy: "flip",            // "clamp" (default) | "flip" | "raw"
  anchor_offset: { x: 14, y: 22 },  // applied BEFORE the policy
  children: [ /* ordinary ogham chrome */ ],
}
```

```rust
// Per frame, or only when the thing moves — anchors are host state,
// not frame state, and persist until changed.
ogham.set_anchor("action-tooltip", Point::new(cursor_x, cursor_y));
ogham.clear_anchor("action-tooltip");   // the thing is gone
ogham.clear_anchors();                  // all of them
let p: Option<Point> = ogham.anchor("action-tooltip");
```

Run `examples/portals/anchored_tooltip.ogh` in the `client` binary
(ctrl+O) and move the mouse; the previewer publishes every pointer
move as the `"cursor"` anchor.

**Why it's a Portal property and not a composition pattern.** The
policies need the subtree's *measured* size, which `.ogh` cannot
see. "Flip above the pointer when the card would overrun the
bottom" is not expressible by any arrangement of widgets, which is
why the rule lives in the renderer, applied after layout.

| Policy | Rule |
|---|---|
| `"clamp"` (default) | Keep the whole box inside the viewport, inset 8 px on every edge. A box too large to fit pins to the top-left inset. |
| `"flip"` | Clamp horizontally; vertically, sit *above* the anchor when the box would overrun the bottom, mirroring `anchor_offset.y`. Then clamp. This is the cursor-tooltip rule. |
| `"raw"` | The point plus the offset, nothing else. May go off-screen — for hosts that did their own edge math. |

**What you get for free.** The anchor resolves into the entry's
`viewport_rect`, which is the same field an unanchored portal
computes, so *everything downstream already works*: it paints
where you anchored it, it's clickable where it's drawn,
`UI::blocks_point` occludes your world picking under it, nested
portals inherit the anchored origin, and layer/backdrop/cursor
policy are unchanged.

**A missing anchor renders nothing.** An `anchor:` id the host
hasn't set means "the thing I was pointing at is gone", so the
portal is skipped for that frame — no error, no fallback position.
Debug builds print one line per id that was never set, so a typo'd
id is distinguishable from a host with nothing to point at.

| | |
|---|---|
| Coordinates | The same viewport coordinates layout runs in — the ones you pass to `frame(width, height, dt)`. Logical pixels, not device. |
| Lifetime | Host state. Persists until `clear_anchor` / `clear_anchors` / a **hot reload** (INTENT §7 — a reload drops anchors, so a host that sets one *once* must re-set it after). |
| World-space chrome | Project world → screen host-side and pass the result. Ogham does not know what a camera is. |
| `focus_trap` | **Rejected at build time** with `anchor`. A focus trap that follows a host-set point can strand input over chrome the user can't reach. |
| Reserved ids | Ids starting with `__` are the runtime's (the drag preview lives at `__drag_preview`) and are rejected in `.ogh`. |
| Unknown `anchor_policy` | `BridgeError::InvalidPropertyType` listing the three valid names — not a silent fall back to the default. |
| Measured size | The **union of the portal's children's** laid-out rects — not the Portal's own rect, which is `grow`/`grow`. Give an anchored Portal *one* content child. A full-viewport backdrop sibling (the `Modal` composition pattern) makes the measured box the viewport, and `clamp` then pins it to the corner. |
| Collision | Policies resolve against the *viewport* only. Two anchored tooltips overlapping is the host's problem. |
| Anchoring to a widget | Not supported. `anchor` takes a host-supplied point, never "the widget with key `foo`". |

### Host-painted `Canvas`

`Canvas` is a **leaf** widget whose pixels are drawn by *your* Rust —
arcs, wedges, gradients along a path, blend modes, anything the typed
`RenderContext` primitives can't express — while its geometry comes
from flex layout like any other widget. Use it when host-drawn content
is **widget-sized and has siblings**. A full-screen host surface under
a transparent Ogham root already works and does not need this.

```ogh
Canvas {
  painter: "wheel_dial",                       // required; names a host painter
  props: { charge: charge, dancing: dancing }, // optional; handed to the painter verbatim
  style: {
    width: 180, height: 194,                   // "grow" / "shrink" also legal
    margin: { bottom: 14 },                    // INSIDE the box: the painter gets 180x180
    cursor: "pointer",
  },
  mouse_down: fn () { event("dial_press"); },  // ordinary pointer listeners
}
```

```rust
use ogham::runtime::config::RuntimeConfig;
use ogham::skia_safe::{Paint, PaintStyle};      // re-exported: use THIS skia-safe

let state = shared_state.clone();               // an Arc<Mutex<…>>, as with event handlers
let config = RuntimeConfig::new()
    .with_painter("wheel_dial", move |p, props| {
        // p.canvas() is pre-translated to the widget's origin and pre-scaled
        // by DPI: draw in LOCAL LOGICAL coordinates from (0, 0).
        let (cx, cy) = (p.width() / 2.0, p.height() / 2.0);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(6.0);            // 6 LOGICAL px on any display
        p.canvas().draw_circle((cx, cy), p.width().min(p.height()) / 2.0 - 6.0, &paint);
        let _ = (&state, props);
    });
```

Run `examples/canvas.ogh` in the previewer (`cargo run -p ogham_preview`
in `../lorekeeper`, Ctrl+O) for a working one; its painter is `demo_dial`
in lorekeeper's `ogham_preview/src/main.rs`.

**The painter contract.**

| | |
|---|---|
| Signature | `Fn(&mut Painter, &Value) + Send + Sync + 'static` |
| Registration | `RuntimeConfig::with_painter(name, f)` — **on the config, never on a live `Runtime`** |
| Name matching | Exact (unlike widget type names, which are lowercased) |
| `props` | The `.ogh` `props:` value verbatim, fresh each frame. Defaults to an empty map. **The only channel from `.ogh` into the painter** — live host data comes from handles the closure captured at registration. |
| Nothing flows out | A painter draws and returns. It has no route back into the runtime; state changes leave `.ogh` as `event(...)` calls, as always. |
| Children | None. `Canvas` is a leaf. Chrome that must sit over painted content is a sibling with `position: { type: "absolute" }`, or a `Portal`. |

**The coordinate space** is the reason this is worth using:

- `p.canvas()` is already `save()`d, translated to the widget's
  laid-out origin, and scaled by the display's DPI factor. Draw from
  `(0, 0)` to `(p.width(), p.height())` in **logical** pixels.
- Sizes, stroke widths, and offsets are logical too — a 2px hairline is
  `2.0`, on any display. `p.dpi_scale()` is there for the rare painter
  that wants device pixels.
- **`margin` is subtracted from the painter's rect; padding and border
  are not.** This is the opposite of CSS and it catches everyone once:
  Ogham's layout rect is `width × height` *including* insets
  ([`FLEX.md`](docs/internal/FLEX.md) §Insets), so
  `width: 180, height: 194, margin: { bottom: 14 }` hands the painter
  **180 × 180**, not 180 × 194 with a gap below. Declare the box as
  *painter size plus the margins you want*, and read `p.width()` /
  `p.height()` rather than assuming they match the style. Padding and
  border are left alone deliberately — the painter owns its whole
  interior.
- The canvas is restored afterwards, so nothing you do to the transform
  leaks into the next widget.
- Migrating a full-screen paint routine is mechanical: replace its
  centre computation with `p.width() / 2.0`, `p.height() / 2.0`, and
  delete every seating constant.

**Hot reload.** Painters registered on `RuntimeConfig` survive a hot
reload for exactly the same reason event handlers do: `Ogham::reload`
rebuilds the `Runtime` *from the config*. A painter poked into a live
`Runtime` after construction would work until the first file save and
then vanish, silently — so there is no API to do that.

**Failure modes** (all loud, deliberately):

| What you did | What happens |
|---|---|
| Omitted `painter:` | `BridgeError::MissingProperty("painter")` at build time. |
| Named a painter that isn't registered | `BridgeError::InvalidPropertyType` at build time, **listing every registered name**. Surfaces through whatever channel your host already shows bridge errors in. |
| Registered on a live `Runtime` instead of the config | Works once, disappears on the next hot reload. Don't. |
| Rendering backend doesn't implement `with_local_canvas` | The `Canvas` paints nothing (its layout still happens) and debug builds log once per painter name. `SkiaEnv` implements it, so this only bites custom backends. |
| Your painter panics | **The panic propagates.** It is host code on the host's render thread; Ogham does not `catch_unwind` it, because swallowing it would hide your bug behind a blank rectangle. |
| Depended on `skia-safe` directly instead of `ogham::skia_safe` | Two copies of the crate in one binary; `p.canvas()` stops typechecking with an error that blames the wrong thing. Use the re-export. |

**Occlusion.** A `Canvas` with a `mouse_down` / `mouse_up` /
`contextmenu` listener consumes presses and reports `true` from
`UI::blocks_point`; a bare one is see-through, so a decorative dial
doesn't swallow clicks meant for the scene behind it. Same rule as
`Flex`.

**Reconciliation.** Same `painter:` name → the widget absorbs in place
(props and style swap, `Arc` identity survives). Different name →
replacement, because a different painter shares no pixels with the old
one.

**`key:` does nothing on a `Canvas`** — as on every other leaf. Only
`Flex` and `Portal` implement `Widget::key`, so a keyed leaf is matched
by position like an unkeyed one, silently. A reorderable list of
Canvases therefore wants each one wrapped in a keyed `Flex`, which is
what actually carries the identity. (This is a real gap, not a design
statement: INTENT §5 makes `key` the identity mechanism, and a leaf that
accepts the property while ignoring it is the silent-degradation shape
§3.5 exists to prevent. Wrapping is the workaround until leaves carry
keys.)

See [`docs/internal/CANVAS_LEAF.md`](docs/internal/CANVAS_LEAF.md) for
the design and [INTENT §6](docs/internal/INTENT.md) for the named
exception this carves out of "`Surface` is the only rendering seam".

### Imports

```ogh
import "./button.ogh";
import [ComponentName] from "./components.ogh";
```

### Control flow

```ogh
if (x > 0) { "positive" } else { "non-positive" };

match value {
  true => selected_bg,
  false => default_bg,
};

for (i in 0..items.length()) {
  render_item(items[i])
};
```

### Debugging

```ogh
log expression;  // prints value to stderr
```

## Strict Vocabulary

Ogham drops what it does not recognise — **keys and values alike**. A
widget with a misspelled property parses, lays out, draws, and is wrong,
with no error and nothing in a test: `on_click` where the listener is
`mouse_down`, `font_size` where the text style key is `size`, `wrap`
where the Flex property is `flex_wrap`, `cross_alignment: "stretch"`
where the closed set has no such member and the builder reads `start`.

`widget::vocabulary` is the vocabulary written down once, beside
`widget::builder`, and two things read it.

**A source scan**, which knows where things are:

```bash
cargo run -p ogham --bin ogham-vocab -- data/ui/*.ogh
```

```rust
let source = std::fs::read_to_string(path)?;
let found = ogham::widget::vocabulary::scan_source(path, &source);
assert!(found.is_empty(), "{found:#?}");   // the shape a repo's test takes
```

It sees what an author literally wrote. A style map reached through a
`let` — the idiomatic shape, because a `{` after `=>` is a block — is
invisible to it, deliberately: guessing there is the false positive that
gets a lint switched off.

**The builder**, which cannot be fooled by that indirection:

```rust
let config = RuntimeConfig::new().with_strict_vocabulary();
let ogham = Ogham::from_source(src, config)?;
assert!(ogham.vocabulary_violations().is_empty());
```

Off by default. Turning it on changes nothing about what is drawn — a key
the builder ignores today goes on being ignored, it is only said out
loud — and nothing fails: findings are collected (de-duplicated, so a
widget rebuilt every frame is one entry) and printed once each. A UI
language that panicked mid-frame over a style key would be worse than the
silence it replaced.

A widget type this crate does not own is checked against nothing, because
a host reads whatever properties it likes off its own widgets.

**Adding a key to `widget::builder` means adding it to the matching table
in `widget::vocabulary`.** The tables mirror one `match` each; a key in
one and not the other turns a working property into a reported violation.

## The startup check

`Chrome::validate(ids)` compares a mounted document against the host it is
mounted in, once, at startup: `screen` blocks against the route table's
ids, and the document's declared `events {}` against the handlers the
instance registered. `lorekeeper`'s `RouterHost::new` calls it, which is
the one place both halves are known.

A route that mounts an editor from another crate declares
`Route::brings_own_document`, and comes out of the screen half — its
surface is in another `.ogh` and the shared one is right not to declare
it.

It reports rather than refuses, and `Chrome::validation()` is what a test
asserts on.

## Integration Guide

This section uses examples from [Untold Lore](../untold_lore), a game client that uses Ogham for all of its UI.

### 1. Initialization

Use `RuntimeConfig` to configure host state and event handlers, then create an `Ogham` instance.

**Simple example** (from Untold Lore's escape menu):

```rust
use ogham::runtime::config::RuntimeConfig;
use ogham::Ogham;

const ESCAPE_UI_PATH: &str = "./data/escape_menu.ogh";

let config = RuntimeConfig::new()
    .with_event_handler("continue", move |_| {
        // handle "continue" event from .ogh
        true
    })
    .with_event_handler("exit_to_main_menu", move |_| {
        // handle "exit_to_main_menu" event from .ogh
        true
    });

let ogham = Ogham::watch(ESCAPE_UI_PATH.to_string(), config)
    .expect("Failed to create Ogham");
```

**With initial host state** (from Untold Lore's multiplayer menu):

```rust
use std::collections::HashMap;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::Ogham;

let initial_state = {
    let mut state = HashMap::new();
    state.insert("is_connected".to_string(), Value::Boolean(false));
    state.insert("is_hosting".to_string(), Value::Boolean(false));
    state.insert("server_name".to_string(), Value::String(String::new()));
    state.insert("player_count".to_string(), Value::Integer(0));
    state.insert("player_list".to_string(), Value::Array(vec![]));
    state
};

let config = RuntimeConfig::new()
    .with_host_state(initial_state)
    .with_event_handler("close", move |_| {
        // handle close
        true
    })
    .with_event_handler("host_game", move |_| {
        // handle host_game
        true
    })
    .with_event_handler("player_name_changed", move |args| {
        if let Some(Value::String(name)) = args.get(0) {
            // use the new player name
        }
        true
    });

let ogham = Ogham::watch("./data/multiplayer_menu.ogh".to_string(), config)
    .expect("Failed to create Ogham");
```

Use `Ogham::from_source()` instead of `Ogham::watch()` when loading UI from a string rather than a file (no hot-reload).

### 2. Injecting state per frame

After initialization, inject updated state each frame via the runtime:

```rust
let runtime = ogham.get_runtime().clone();
let mut rt = runtime.lock().unwrap();

rt.inject_host_state("is_connected".to_string(), Value::Boolean(true));
rt.inject_host_state("player_count".to_string(), Value::Integer(4));

// For complex data, build Value::Map or Value::Array:
let players: Vec<Value> = player_list.iter().map(|p| {
    let mut map = HashMap::new();
    map.insert("id".to_string(), Value::Integer(p.id as i32));
    map.insert("name".to_string(), Value::String(p.name.clone()));
    Value::Map(map)
}).collect();
rt.inject_host_state("player_list".to_string(), Value::Array(players));

// Use inject_host_state_if_changed to skip no-op updates:
rt.inject_host_state_if_changed("server_name".to_string(), Value::String(name));

rt.request_rerender();
```

Host state values are accessible as top-level variables in `.ogh` files. For example, after injecting `"player_count"`, the `.ogh` file can reference `player_count` directly.

### 3. Event handling

Events flow from `.ogh` files to Rust via `event("name", args...)`. Register handlers with `with_event_handler`:

```rust
.with_event_handler("tool_selected", move |args| {
    if let Some(Value::String(tool)) = args.get(0) {
        // react to tool selection
    }
    true // return true if the event was handled
})
```

Handlers receive `&[Value]` and return `bool`. A common pattern is to set flags or push commands onto a shared queue (`Arc<Mutex<Vec<Command>>>`), which the main loop processes later.

### 4. Hot-reload loop

When using `Ogham::watch()`, check for file changes each frame:

```rust
if ogham.check_for_changes() {
    if let Err(err) = ogham.reload() {
        eprintln!("Error reloading: {}", err);
    }
}
```

The watcher monitors both the main `.ogh` file and all imported files.

### 5. Rendering pipeline

Call `update()` each frame to check whether a rerender is needed and, if so,
re-execute the module, bridge the widget values, and reconcile the tree.
It returns `Ok(true)` when a rerender was performed (useful for dirty-tracking).
`update()` also runs `process_drain_queues` after reconcile so any pending
drain-time unmount / effect-cleanup hooks fire in the same frame.

```rust
// 1. Update (rerender + bridge + reconcile + drain, only if needed)
let rerendered = ogham.update()?;

// 2. Tick animations forward by the real elapsed time (in seconds).
//    Required for style transitions, entry/exit animations, Presence
//    sequencing, AND the drain-time unmount machinery (settled exits
//    surface their owning paths to UI's pending vecs here). Cap dt to
//    ~33ms so a stalled frame doesn't skip through a whole animation
//    in one step.
ogham.get_ui_mut().tick_animations(dt);

// 3. Layout (also runs the drag-preview's layout pass if a drag is
//    in flight — see Drag dispatch below)
ogham.get_ui_mut().layout(window_width, window_height);

// 4. Draw using a Surface implementation
surface.draw(ogham.get_ui_mut());

// 5. Optional: drain again. tick_animations may have queued drain
//    prefixes (settled exit animations); call process_drain_queues
//    to flush their unmount hooks before the next frame starts.
ogham.process_drain_queues();
```

**Important**: `tick_animations` must run *every* frame, not just when `update()`
reports a rerender. Animations are driven by per-frame ticks independent of
runtime state changes. Skipping ticks freezes any in-flight transitions and
defers drain-time unmount hooks indefinitely.

### 6. Handling UI events

Route input events (clicks, keyboard) to the UI tree:

```rust
use ogham::widget::event::Event;
use ogham::widget::point::Point;

let event = Event::with_point("mouse_down".to_string(), Point::new(x, y));
ogham.get_ui_mut().call_event(&event);
```

For the dedicated drag + contextmenu paths added in Phase 3, the
host's input pump translates its dead-zone state machine into the
`dispatch_drag_*` / `dispatch_contextmenu` calls — see *Drag
dispatch* below.

### 7. Drag dispatch (Phase 3)

The host owns the dead-zone state machine; Ogham provides the
event-emission and drop-target hit-test. Typical flow:

```rust
use ogham::widget::event::DragState;

// On mouse_down past the per-widget dead-zone (4px default):
let target = ogham.get_ui_mut().hit_test_drag_target(&Point::new(x, y));
let mut drag_state = if let Some(origin) = target {
    Some(ogham.dispatch_drag_start(origin, payload, Point::new(x, y)))
} else { None };

// Each subsequent mouse_move while a drag is in flight:
if let Some(state) = drag_state.as_mut() {
    ogham.dispatch_drag_move(state, Point::new(x, y));
}

// On mouse_up:
if let Some(state) = drag_state.as_mut() {
    ogham.dispatch_drag_end(state, Point::new(x, y));
}
drag_state = None;
```

`dispatch_drag_end` walks open portal layers (high → low) then
the base tree, picking the deepest widget whose `accepts_drop`
predicate returns true. If none accept, `drag_end` fires on the
originator. The drag preview (if the source widget declared
`drag_preview:`) renders into the `cursor-attached` portal layer
at the cursor position; clear it from the host on drag end.

`Ogham::hit_test_drop_target(payload, point)` is exposed for
hover-style highlighting (e.g. "is the cursor currently over a
valid drop target?") without firing an event.

For right-click → contextmenu:

```rust
ogham.dispatch_contextmenu(Point::new(x, y));
```

### 8. Cursor and key signals

Three Ogham-side accessors let the host coordinate input
without manually tracking modal / focus state:

```rust
// True if any open portal has focus_trap: true. Use as a single
// source of truth for "should game input be blocked?".
let blocked = ogham.has_input_blocking_portal();

// True if any active portal in overlay-modal / popover OR the
// focused widget declares cursor-free. Use to release a locked
// pointer cursor.
let cursor_free = ogham.wants_cursor_free();

// True if the focused widget consumes Key::Character(_) events
// (TextInput is the canonical case). Use BEFORE feeding the key
// into game pressed() / held() queries so typing into a field
// doesn't trigger hotkeys.
let typing = ogham.consumes_character_key();
```

### 9. Custom rendering backends

Two traits live in `src/widget/mod.rs`:

- **`Surface`** — entry point. Implementations walk the widget tree and call each widget's `render()` / `post_render()` methods. Its only required method is `draw(&mut self, ui: &mut UI)`.
- **`RenderContext`** — the drawing API widgets call from inside their `render()` methods. Implementations provide primitives like `fill_rect`, `draw_border`, `draw_text`, plus stack-based scopes via `push_clip_rect` / `pop_clip_rect`, `push_effects` / `pop_effects`, and `push_backdrop_blur` / `pop_backdrop_blur`.

```rust
pub trait Surface {
    fn draw(&mut self, ui: &mut UI);
}

pub trait RenderContext {
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: &Color);
    fn fill_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radii: &CornerRadii, color: &Color);
    fn draw_border(&mut self, border: &Border, x: f32, y: f32, w: f32, h: f32, radii: &CornerRadii);
    fn draw_image(&mut self, path: &str, x: f32, y: f32, w: f32, h: f32, cache: &mut ImageCache);
    fn draw_text(&mut self, text: &str, style: &TextStyle, x: f32, y: f32, width: f32);
    fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: &Color);
    fn draw_svg_dom(&mut self, dom: &skia_safe::svg::Dom, x: f32, y: f32, w: f32, h: f32);

    fn push_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {}
    fn pop_clip_rect(&mut self) {}

    /// Push an opacity layer (`< 1.0` triggers offscreen compositing) and an
    /// affine transform pivoting around `(pivot_x, pivot_y)`. Paired with
    /// `pop_effects`. Required for opacity / transform style transitions to
    /// render correctly.
    fn push_effects(&mut self, opacity: f32, transform: &Transform, pivot_x: f32, pivot_y: f32) {}
    fn pop_effects(&mut self) {}

    /// Begin a backdrop-filter layer over `(x, y, w, h)` clipped to `radii`,
    /// applying a Gaussian blur of `sigma` to the canvas content already
    /// painted under the rect. Backends without backdrop sampling can leave
    /// the no-op default — the panel still renders, just without the
    /// frosted-glass capture. Always paired with `pop_backdrop_blur`.
    fn push_backdrop_blur(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        radii: &CornerRadii, sigma: f32,
    ) {}
    fn pop_backdrop_blur(&mut self) {}

    /// The `Canvas` painter hatch: hand `paint` a backend-native canvas
    /// positioned at `rect`'s origin and scaled to device pixels, so a
    /// host painter draws in local logical coordinates from (0, 0) to
    /// (rect.width, rect.height). Save before, restore after. Backends
    /// that can't expose a native canvas keep the `false` default and the
    /// Canvas widget paints nothing. See *Host-painted `Canvas`* above.
    fn with_local_canvas(
        &mut self, rect: &Rect,
        paint: &mut dyn FnMut(&mut canvas_widget::Painter),
    ) -> bool { false }
}
```

**Two-pass rendering.** Backends draw the base tree first
(Pass A), collecting any `Portal` widgets as `PortalEntry`s on
`UI::portal_layers`. After Pass A returns, the backend walks
each open portal layer in priority order
(`overlay-modal` → `popover` → `tooltip` → `toast` →
`cursor-attached`) and paints its entries at viewport-absolute
coordinates (Pass B), applying the layer's `BackdropPolicy`
before painting children. A Portal that declares an `anchor:`
takes its Pass-A origin from `UI`'s anchor map instead of from
accumulated translates, and is skipped entirely when the host
hasn't set that id. While a drag is in flight, the backend
synthesizes a `cursor-attached` entry from
`UI::active_drag_preview`, seated at the runtime-reserved
`__drag_preview` anchor through that same path. See
[`docs/internal/SURFACE.md`](docs/internal/SURFACE.md) for
the full walker description.

`SkiaEnv` in `src/skia.rs` is the reference implementation of both traits.

## Project Structure

```
ogham/
  Cargo.toml
  README.md
  AGENTS.md
  rustfmt.toml
  src/
    lib.rs                  -- library entry point, Ogham struct
    file_watcher.rs         -- hot-reload file watching
    skia.rs                 -- Skia rendering backend
    scanner/                -- lexical analysis
    parser/                 -- AST construction
    runtime/
      mod.rs                -- Runtime core
      compiler.rs           -- bytecode compiler
      vm.rs                 -- virtual machine
      value.rs              -- Value enum
      config.rs             -- RuntimeConfig builder
      environment.rs        -- variable scoping
      error.rs              -- error types
      opcode.rs             -- bytecode opcodes
      ops.rs                -- arithmetic/comparison operations
      descriptor.rs         -- WidgetDescriptor (runtime widget representation)
    widget/
      mod.rs                -- UI struct, Surface trait, Widget trait, RenderEffects, TickResult
      builder.rs            -- runtime values -> widget tree nodes
      flex_widget.rs        -- Flex container; transitions + exit lifecycle + drag fields
      text_widget.rs        -- Text display
      text_input_widget.rs  -- Text input
      svg_widget.rs         -- SVG rendering
      grid_widget.rs        -- Grid container
      image_widget.rs       -- Image
      canvas_widget.rs      -- Host-painted Canvas leaf (Painter + the paint escape)
      presence_widget.rs    -- Lifecycle-sequencing container
      portal_widget.rs      -- Portal (deferred-paint two-pass renderer)
      portal_layer.rs       -- Named portal layers + backdrop / cursor policies
      animation.rs          -- Spring math + per-property animation state
      event.rs              -- Event, EventContext, TickContext, DragState
    cli/                    -- `ogham` CLI (check subcommand; render.rs is the diagnostic formatter)
    lsp/                    -- `ogham-lsp` language server
    diagnostics/            -- schema-diagnostic engine (used by CLI + LSP)
    typed.rs                -- TypedOgham handle (typed host state + msg)
    client/
      main.rs               -- standalone browser binary entry point
      client.rs              -- client implementation
      app.rs                 -- application wrapper
  crates/
    ogham-derive/           -- #[derive(OghamState)] / #[derive(OghamMsg)] proc macros
  examples/                  -- .ogh example files (incl. portals/components.ogh)
  tests/                     -- integration tests
  docs/internal/             -- design docs + per-phase implementation trailers
```
