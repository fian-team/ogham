# Ogham

Ogham is a UI language and embeddable runtime for Rust applications. The
runtime compiles `.ogh` source to bytecode, executes it on a stack-based VM,
and reconciles the resulting widget tree against a live UI. A Skia-based
backend ships out of the box; alternative backends implement the
`Surface` trait.

This document is the contributor onboarding map. Library users embedding
Ogham in a host application should read [`AGENTS.md`](AGENTS.md) for the
integration guide. The design contracts behind each subsystem live in
[`docs/internal/`](docs/internal/).

## Repository layout

```
ogham/
├─ src/
│  ├─ lib.rs                  — top-level Ogham facade (Arc<Mutex<Runtime>> + UI)
│  ├─ scanner/                — lexer (.ogh source → tokens)
│  ├─ parser/                 — recursive-descent parser (tokens → AST)
│  ├─ runtime/                — Runtime, RuntimeConfig, bytecode compiler, VM
│  ├─ widget/                 — UI struct, widget tree, Surface trait, layout, animation
│  ├─ skia.rs                 — reference Skia backend
│  ├─ typed.rs                — TypedOgham<S, M> wrapper for derive-based bindings
│  ├─ diagnostics/            — schema-diagnostic engine (consumed by `ogham check`)
│  ├─ cli/                    — `ogham` CLI binary
│  ├─ lsp/                    — `ogham-lsp` language server binary
│  ├─ client/                 — standalone .ogh viewer binary
│  └─ file_watcher.rs         — hot-reload file watching
├─ crates/
│  └─ ogham-derive/           — proc-macros: #[derive(OghamState)], #[derive(OghamMsg)]
├─ examples/                  — runnable .ogh programs
├─ editors/vscode/            — VS Code extension (talks to ogham-lsp)
├─ tests/                     — integration tests (cargo test)
└─ docs/internal/             — design contracts and per-phase implementation plans
```

The crate is a Cargo workspace. `crates/ogham-derive` is split out so the
proc-macro can be a `proc-macro = true` crate without forcing the rest of
the library to compile in that mode.

## Binaries

The workspace produces two binaries plus the `ogham` library crate:

| Binary       | Path                | Purpose                                                           |
|--------------|---------------------|-------------------------------------------------------------------|
| `ogham`      | `src/cli/main.rs`   | CLI. Currently one subcommand: `check` (validates `.ogh` files against typed-bindings manifests). |
| `ogham-lsp`  | `src/lsp/main.rs`   | Language Server (LSP over stdio). Used by the VS Code extension and any LSP-capable editor. |

## Quick start

```sh
# Build everything (library + 2 binaries).
cargo build

# Preview an .ogh file (hot-reloads on save). The previewer lives in the
# lorekeeper workspace and runs on the engine's own window host:
#   cd ../lorekeeper && cargo run -p ogham_preview -- ../ogham/examples/counter.ogh

# Validate every .ogh file in the workspace against host bindings.
cargo run --bin ogham -- check --all

# Or validate a single file:
cargo run --bin ogham -- check path/to/ui.ogh

# Build and run the LSP (most editors discover it via $PATH).
cargo build --bin ogham-lsp
./target/debug/ogham-lsp        # speaks LSP over stdin/stdout

# Run the test suite.
cargo test
```

The test suite (`tests/*.rs`) doubles as the acceptance suite for the
language and the runtime. Drag, lifecycle, portal, hot-reload, and typed-
binding behaviors all have dedicated test files.

## Embedding the library

Add `ogham` as a dependency, build a `RuntimeConfig`, and call
`Ogham::watch` (file-backed, hot-reload) or `Ogham::from_source`
(string-backed, no watcher). Per frame, drive the runtime with `tick`,
`tick_animations`, `layout`, and `draw`.

For typed host state and event handling via Rust types rather than
`HashMap<String, Value>`, derive `OghamState` and `OghamMsg` and use
`TypedOgham::watch_typed`. The derive macros also emit a manifest fragment
the `ogham check` CLI uses to flag stale `.ogh` references at analysis
time.

The full integration recipe — `RuntimeConfig` builder, host-state
injection, event handlers, hot-reload loop, animation tick, layout pass,
draw, drag dispatch, contextmenu, drain queues, cursor/key signals, and
custom rendering backends — lives in [`AGENTS.md`](AGENTS.md).

## Documentation

- [`AGENTS.md`](AGENTS.md) — integration guide for library consumers and
  language overview for `.ogh` authors.
- [`docs/internal/ARCHITECTURE.md`](docs/internal/ARCHITECTURE.md) —
  one-page orientation: pipeline, embedding seam, per-frame loop, module
  layout, where state lives.
- [`docs/internal/SUBSYSTEMS.md`](docs/internal/SUBSYSTEMS.md) — subsystem
  map: invariants, drift indicators, authority files for each piece.
- [`docs/internal/INTENT.md`](docs/internal/INTENT.md) — cross-cutting
  design tenets and the rationale behind each.
- [`docs/internal/GLOSSARY.md`](docs/internal/GLOSSARY.md) — vocabulary
  used across the contributor docs.
- Per-subsystem live contracts: `LANGUAGE.md`, `VM.md`, `RUNTIME.md`,
  `WIDGET_TREE.md`, `FLEX.md`, `STYLE_AND_ANIMATION.md`,
  `ANIMATION_LIFECYCLE.md`, `EVENTS.md`, `SURFACE.md`, `LSP.md`.
- Phase implementation trailers (typed bindings, lifecycle + portal,
  Phase 2.5 UL alignment, Phase 3 drag + drain) — same directory.

If a doc disagrees with code, the code wins, but flag the discrepancy —
the design discipline behind these contracts depends on drift being made
visible. See `INTENT.md` for the convention.

## Coding conventions

- Rust 2021, MSRV 1.80.
- `rustfmt.toml`: `max_width = 100`. `cargo fmt` before pushing.
- `cargo clippy --all-targets` should pass cleanly.
- Inline unit tests in `#[cfg(test)]` modules; integration tests in
  `tests/`.
- Public items get `///` doc comments; modules get `//!` headers. Comments
  on internal items are reserved for non-obvious *why* — naming carries
  *what*.
- `Runtime` is shared as `Arc<Mutex<Runtime>>`. The interior is
  single-threaded; thread safety comes from the wrapper, not the runtime
  itself.

## Editor support

The VS Code extension in `editors/vscode/` talks to `ogham-lsp` over
stdio. After building the binary, set `ogham.lspPath` in the extension's
settings (defaults to `ogham-lsp` on `$PATH`). Capabilities today:
diagnostics (scanner + parser + typed-bindings AST validation +
lifecycle conditional-registration warnings), hover, go-to-definition,
document symbols, semantic tokens. Completion, find-references, and
rename are not implemented; see
[`docs/internal/LSP.md`](docs/internal/LSP.md) for the roadmap.

## Contributing

Bug reports and pull requests are welcome on the project's issue tracker.
Before submitting non-trivial work, please read
[`docs/internal/INTENT.md`](docs/internal/INTENT.md) — the design tenets
there are load-bearing and the most common review feedback is "this
violates `INTENT §N`". Phase-scoped implementation plans in
`docs/internal/PHASE_*.md` capture the contracts that recently-shipped
work was held against; they are good models for new proposals.

Discord: <https://discord.gg/JYfC2baP2y> (Fian Dev community).

## License

MIT. See [`LICENSE`](LICENSE).
