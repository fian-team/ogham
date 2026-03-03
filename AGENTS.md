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
  -> ast_bridge (src/tree/ast_bridge.rs) -- converts to widget tree nodes
  -> UI (src/tree/mod.rs)               -- layout, reconciliation, events
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
| `src/tree/mod.rs` | `UI` struct, `Surface` trait, `Widget` trait |
| `src/tree/ast_bridge.rs` | Converts runtime widget values to tree nodes |
| `src/tree/flex_widget.rs` | Flexbox container widget |
| `src/tree/text_widget.rs` | Text display widget |
| `src/tree/text_input_widget.rs` | Text input widget |
| `src/tree/svg_widget.rs` | SVG widget |
| `src/tree/event.rs` | Event types (`Event`, `EventContext`) |
| `src/skia.rs` | Skia rendering backend (implements `Surface`) |
| `src/client/` | Standalone browser binary |
| `src/file_watcher.rs` | File watching for hot-reload |

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
| `Widget(RuntimeWidget)` | | A widget produced during execution |
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

Four built-in widget types: `Flex`, `Text`, `TextInput`, `Svg`.

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

### Type casting

```ogh
let s = count -> string;
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

// 2. Layout
ogham.get_ui_mut().layout(window_width, window_height);

// 3. Draw using a Surface implementation
surface.draw(ogham.get_ui_mut());
```

### 6. Handling UI events

Route input events (clicks, keyboard) to the UI tree:

```rust
use ogham::tree::event::Event;

let event = Event::new("click".to_string())
    .with_point(x, y);

ogham.get_ui_mut().call_event(&event);
```

### 7. Custom rendering backends

Implement the `Surface` trait (`src/tree/mod.rs`) to use a renderer other than Skia:

```rust
pub trait Surface {
    fn draw(&mut self, ui: &mut UI);
    fn draw_widget(&mut self, widget: &WidgetRef, focused: Option<&WidgetRef>, image_cache: &mut ImageCache);
    fn draw_box(&mut self, widget: &FlexWidget, image_cache: &mut ImageCache);
    fn draw_borders(&mut self, widget: &FlexWidget, x: f32, y: f32, width: f32, height: f32);
    fn draw_text(&mut self, widget: &TextWidget);
    fn draw_text_input(&mut self, widget: &TextInputWidget);
    fn draw_svg(&mut self, widget: &SvgWidget);
}
```

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
      widget.rs             -- runtime widget representation
    tree/
      mod.rs                -- UI struct, Surface trait, Widget trait
      ast_bridge.rs         -- runtime values -> widget tree nodes
      flex_widget.rs        -- Flex container
      text_widget.rs        -- Text display
      text_input_widget.rs  -- Text input
      svg_widget.rs         -- SVG rendering
      event.rs              -- Event, EventContext
    client/
      main.rs               -- standalone browser binary entry point
      client.rs              -- client implementation
      app.rs                 -- application wrapper
  examples/                  -- .ogh example files
  tests/                     -- integration tests
```
