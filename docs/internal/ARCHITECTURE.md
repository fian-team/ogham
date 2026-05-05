# Ogham — Architecture at a Glance

> **Status: Live contract.**
>
> A one-page orientation. Each diagram answers a question someone
> new to the codebase tends to ask in their first hour. The detail
> lives in the documents this one points at; this page is for the
> picture-shaped answers.
>
> If a diagram contradicts code, the code is the more reliable
> source — but flag the discrepancy. The same drift discipline
> that governs `INTENT.md` applies here.

## 1. The pipeline

```mermaid
flowchart LR
    src[".ogh source"]
    tok["Token stream<br/>(scanner/)"]
    ast["AST<br/>(parser/)"]
    bc["Bytecode<br/>FunctionProto<br/>(runtime/compiler.rs)"]
    val["Values<br/>incl. WidgetDescriptor<br/>(runtime/vm.rs)"]
    tree["Widget tree<br/>WidgetRef<br/>(widget/builder.rs)"]
    ui["UI tick<br/>layout · events · animation<br/>(widget/mod.rs)"]
    px["Pixels<br/>(skia.rs / custom Surface)"]

    src --> tok --> ast --> bc --> val --> tree --> ui --> px
```

Each arrow is a one-way dependency. The compiler doesn't see
runtime state; the VM doesn't construct widget trees; the builder
doesn't see AST; the surface doesn't mutate the tree. This is
load-bearing — see [INTENT §1](INTENT.md#1-the-pipeline-is-four-distinct-passes-not-three).

**Read next:** [`LANGUAGE.md`](LANGUAGE.md) for scanner+parser,
[`VM.md`](VM.md) for compiler+VM, [`WIDGET_TREE.md`](WIDGET_TREE.md)
for builder+reconciliation.

## 2. Embedding

```mermaid
flowchart TB
    subgraph host["Host Rust application"]
        rust["application code"]
        cfg["RuntimeConfig<br/>· host_state<br/>· event_handlers<br/>· custom_widgets<br/>· fonts"]
    end

    subgraph ogham["Ogham instance (lib.rs)"]
        rt["Runtime<br/>(runtime/mod.rs)"]
        uitree["UI<br/>(widget/mod.rs)"]
        fw["FileWatcher<br/>(file_watcher.rs)"]
    end

    surface["Surface impl<br/>(skia.rs or custom)"]

    rust -- "RuntimeConfig::new()<br/>.with_host_state()<br/>.with_event_handler()" --> cfg
    cfg -- "Ogham::watch / from_source" --> rt
    rt -- "VM produces Value::Widget" --> uitree
    fw -. "file changed → reload" .-> rt
    rust -- "tick(inject)<br/>set_screen_size()<br/>call_event()" --> ogham
    rust -- "event(name, ...)<br/>(via handler)" --- rt
    uitree -- "UI::layout(w, h)" --> uitree
    surface -- "draw(&mut UI)" --> uitree
```

Host writes; Ogham reads. From the `.ogh` side, host state is
observable but immutable; mutations leave as `event(...)` calls
the host's registered handlers receive. See
[INTENT §2](INTENT.md#2-host-state-flows-in-events-flow-out) and
[`RUNTIME.md`](RUNTIME.md).

The `tick(inject)` callback is the canonical per-frame seam: the
host pushes the current frame's state into the runtime via the
`inject` closure; `tick` then checks for file changes, runs the
injection, and reconciles the tree if a rerender is pending. Returns
`true` if the tree was reconciled.

## 3. Per-frame loop

```mermaid
sequenceDiagram
    autonumber
    participant Host
    participant Ogham
    participant RT as Runtime
    participant UI
    participant SF as Surface

    Host->>Ogham: tick(inject)
    Ogham->>Ogham: file_watcher.check_for_changes()
    alt watcher reports change
        Ogham->>RT: reload (fresh Runtime from disk)
    end
    Ogham->>RT: inject(&mut runtime) — host state push
    Note over RT: set_host_state diffs;<br/>request_rerender if changed
    Ogham->>RT: needs_rerender()?
    alt yes
        RT->>RT: rerender() → Value::Widget
        Ogham->>UI: reconcile(new_root)
        UI-->>Ogham: UpdateResult { needs_layout, needs_repaint }
    end
    Host->>Ogham: tick_animations(dt)
    Ogham->>UI: tick_animations(dt) — springs + smooth scroll
    Host->>Ogham: layout(w, h)
    Ogham->>UI: UI::layout(w, h) — runs only if dirty
    Host->>SF: draw(&mut ui)
    SF->>UI: walk tree, calling Widget::render
```

Three independent dirty signals reach the UI: rerenders
(`mark_needs_layout`), event handling (`mark_dirty`), and
animation ticks. Each can request a layout pass; debug builds
attribute layout calls per source so a chronic over-marking
regression shows up as a per-second warning.

**Read next:** [`WIDGET_TREE.md`](WIDGET_TREE.md) for what
`reconcile` actually does, [`STYLE_AND_ANIMATION.md`](STYLE_AND_ANIMATION.md)
for `tick_animations`, [`SURFACE.md`](SURFACE.md) for the draw seam.

## 4. Module layout

```mermaid
flowchart TB
    subgraph src["src/"]
        lib["lib.rs<br/>Ogham (top-level facade)"]

        subgraph scanner["scanner/"]
            sc["mod.rs · token.rs · token_type.rs"]
        end

        subgraph parser["parser/"]
            pa["mod.rs (Parser)<br/>statement.rs · expression.rs<br/>literal.rs · widget.rs · function.rs<br/>operator.rs · span.rs · syntax_error.rs<br/>array.rs · map.rs · call.rs · block.rs · identifier.rs · node.rs"]
        end

        subgraph runtime["runtime/"]
            rt["mod.rs (Runtime, StateManager, ImportResolver)<br/>config.rs · host_state.rs · environment.rs<br/>compiler.rs · vm.rs · opcode.rs<br/>value.rs · ops.rs · descriptor.rs · error.rs"]
        end

        subgraph widget["widget/"]
            w["mod.rs (UI, Widget trait, Surface trait)<br/>builder.rs (Value → tree)<br/>flex_widget.rs · presence_widget.rs<br/>grid_widget.rs · text_widget.rs<br/>text_input_widget.rs · svg_widget.rs<br/>image_widget.rs<br/>style.rs · animation.rs · event.rs<br/>point.rs · rect.rs · image.rs"]
        end

        subgraph lsp["lsp/ (binary: ogham-lsp)"]
            l["server.rs · document.rs · hover.rs<br/>goto_definition.rs · semantic_tokens.rs<br/>document_symbols.rs · main.rs"]
        end

        subgraph client["client/ (binary: ogham browser)"]
            c["standalone .ogh viewer"]
        end

        skia_mod["skia.rs<br/>(SkiaEnv : Surface)"]
        fw["file_watcher.rs"]
    end

    lib --> scanner
    lib --> parser
    lib --> runtime
    lib --> widget
    lib --> skia_mod
    lib --> fw
    runtime --> parser
    runtime --> scanner
    widget --> runtime
    skia_mod --> widget
    lsp --> scanner
    lsp --> parser
    client --> lib
```

The LSP depends on `scanner` and `parser` only — never on
`runtime` or `widget`. That's the seam that keeps it cheap and
side-effect-free; see [LSP.md](LSP.md) and
[INTENT §1](INTENT.md#1-the-pipeline-is-four-distinct-passes-not-three).

## 5. Where state lives

```mermaid
flowchart LR
    subgraph runtime["Runtime (one per Ogham instance)"]
        hs["host_state<br/>HashMap&lt;String, Value&gt;"]
        cs["state.component_state<br/>HashMap&lt;path:name, Value&gt;"]
        eh["event_handlers"]
        ctx["context_stack<br/>(transient per render)"]
        env["environment<br/>(transient per render)"]
        comp["compiled_module<br/>(cached bytecode)"]
    end

    subgraph ui["UI (widget tree)"]
        sp["per-widget springs<br/>(AnimationState)"]
        hov["per-widget hover"]
        sc["per-Flex scroll_y"]
        focus["focused widget"]
    end

    rust["Host Rust app"] -- write --> hs
    runtime -- "VM reads via GetState (with fallback)" --- env
    runtime -- "VM reads via GetHostState" --- hs
    runtime -- "DeclareState/SetState writes" --- cs
    ui -- "preserved across reconcile" --- sp
    ui -- "preserved across reconcile" --- hov
    ui -- "preserved across reconcile" --- sc

    classDef volatile fill:#ffeebb,stroke:#aa8833;
    class env,ctx,comp volatile;
```

Yellow boxes are derived/transient and rebuilt every render (the
environment and context stack are cleared at the start of every
module execution; the compiled module is cached but invalidated
when the source changes). Everything else persists across
rerenders — that persistence is what makes spring animations and
state-driven UI work.

**Watch out:** `Ogham::reload` and `recompile_from_source` build
a *new* `Runtime`, so component state is dropped on hot reload.
Widget tree state survives because it lives on the `UI`, which
reconciles against the new descriptors. See
[INTENT §7](INTENT.md#7-hot-reload-preserves-what-it-can-drops-what-it-cant).
