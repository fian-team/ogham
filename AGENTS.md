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
| `src/widget/presence_widget.rs` | Lifecycle-sequencing container (waits for exits before mounting next generation) |
| `src/widget/animation.rs` | Spring math + per-property animation state used by transitions |
| `src/widget/event.rs` | Event types (`Event`, `EventContext`) |
| `src/skia.rs` | Skia rendering backend (implements `Surface`) |
| `src/client/` | Standalone browser binary |
| `src/file_watcher.rs` | File watching for hot-reload |

## LSP Server

The `ogham-lsp` binary implements the Language Server Protocol for `.ogh` files,
enabling editor support and Claude-assisted development.

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
| `Void` | | Unit / no value |

When injecting state from Rust, build values using these constructors directly, e.g. `Value::String("hello".to_string())`, `Value::Integer(42)`, `Value::Array(vec![...])`, `Value::Map(map)`.

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

Built-in widget types: `Flex`, `Text`, `TextInput`, `Svg`, `Image`, `Grid`, `Presence`. `Flex` is the workhorse — it owns the layout/style/animation/lifecycle machinery; the other containers (`Grid`, `Presence`) wrap or specialize it.

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
as `mouse_down`, which makes drag-drop expressible: `mouse_down` on
the source starts the drag, `mouse_enter` / `mouse_leave` on
candidates highlight valid drop targets, `mouse_up` on the target
completes the drop. `TextInput` exposes `mouse_down` and `mouse_up`;
`Image` exposes `mouse_down`.

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

#### `Presence` — sequencing transitions between generations

`Flex`'s exit animations run *in parallel* with the new content mounting. When you want one to finish before the other starts (e.g., page transitions), wrap the swap point in `Presence`:

```ogh
Presence {
  key: current_route_id,
  children: [ render_route(current_route_id) ],
}
```

When `key` changes, every current child is asked to `begin_exit` (cascading through descendants if no own `exit` exists). Children with exit animations stay in the tree as ghosts; the new content is held aside as *pending* and mounts only once all ghosts settle. Rapid key changes replace the pending content latest-wins; reverting the key mid-exit cancels the transition and unwinds the in-flight exits.

Use `Presence` for route boundaries; nest one per "slot" that should sequence independently (e.g., separate Presences for a sidebar and a main panel).

#### Opacity and transform style properties

`opacity` (number 0..1, default 1) and `transform` (`{ translate_x, translate_y, scale, scale_x, scale_y, rotate }`, default identity) are paint-only — they don't affect layout or hit-testing. `transform` pivots around the widget's center. Both are spring-animatable when listed in `transition:`.

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

```rust
// 1. Update (rerender + bridge + reconcile, only if needed)
let rerendered = ogham.update()?;

// 2. Tick animations forward by the real elapsed time (in seconds).
//    Required for style transitions, entry/exit animations, and Presence
//    sequencing to advance. Cap dt to ~33ms so a stalled frame doesn't
//    skip through a whole animation in one step.
ogham.get_ui_mut().tick_animations(dt);

// 3. Layout
ogham.get_ui_mut().layout(window_width, window_height);

// 4. Draw using a Surface implementation
surface.draw(ogham.get_ui_mut());
```

**Important**: `tick_animations` must run *every* frame, not just when `update()`
reports a rerender. Animations are driven by per-frame ticks independent of
runtime state changes. Skipping ticks freezes any in-flight transitions.

### 6. Handling UI events

Route input events (clicks, keyboard) to the UI tree:

```rust
use ogham::widget::event::Event;

let event = Event::new("click".to_string())
    .with_point(x, y);

ogham.get_ui_mut().call_event(&event);
```

### 7. Custom rendering backends

Two traits live in `src/widget/mod.rs`:

- **`Surface`** — entry point. Implementations walk the widget tree and call each widget's `render()` / `post_render()` methods. Its only required method is `draw(&mut self, ui: &mut UI)`.
- **`RenderContext`** — the drawing API widgets call from inside their `render()` methods. Implementations provide primitives like `fill_rect`, `draw_border`, `draw_text`, plus stack-based scopes via `push_clip_rect` / `pop_clip_rect` and `push_effects` / `pop_effects`.

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
}
```

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
      flex_widget.rs        -- Flex container; transitions + exit lifecycle
      text_widget.rs        -- Text display
      text_input_widget.rs  -- Text input
      svg_widget.rs         -- SVG rendering
      grid_widget.rs        -- Grid container
      image_widget.rs       -- Image
      presence_widget.rs    -- Lifecycle-sequencing container
      animation.rs          -- Spring math + per-property animation state
      event.rs              -- Event, EventContext
    client/
      main.rs               -- standalone browser binary entry point
      client.rs              -- client implementation
      app.rs                 -- application wrapper
  examples/                  -- .ogh example files
  tests/                     -- integration tests
```
