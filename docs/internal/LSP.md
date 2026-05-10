# Ogham — LSP Server (`ogham-lsp`)

> **Status: Live contract.**
>
> The Language Server Protocol implementation that ships with
> Ogham. Provides hover, goto-definition, document symbols,
> semantic tokens, and basic diagnostics for `.ogh` files. This
> doc covers what the server does and what's load-bearing about
> *how* it does it.

---

## Authority

- Server entry: [`src/lsp/main.rs`](../../src/lsp/main.rs)
  (`tokio::main`, stdio transport, `tower-lsp`).
- LSP capabilities + dispatch:
  [`src/lsp/server.rs`](../../src/lsp/server.rs).
- Per-document scan + parse:
  [`src/lsp/document.rs`](../../src/lsp/document.rs).
- Per-feature analyzers:
  [`src/lsp/hover.rs`](../../src/lsp/hover.rs),
  [`src/lsp/goto_definition.rs`](../../src/lsp/goto_definition.rs),
  [`src/lsp/document_symbols.rs`](../../src/lsp/document_symbols.rs),
  [`src/lsp/semantic_tokens.rs`](../../src/lsp/semantic_tokens.rs).

---

## At a glance

```
.ogh edit
  → did_open / did_change (tower-lsp)
  → DocumentStore.open / get_mut
    Document::analyze:
      Scanner → tokens
      Parser  → ast (best-effort; None on syntax error)
      parse_schema(ast) → schema_error (typed-bindings AST
                                       validation — Phase 1)
  → collect_diagnostics:
      scanner Error tokens                → Diagnostic (Error)
      parser SyntaxError                  → Diagnostic (severity from err)
      doc.schema_error (typed-bindings)   → Diagnostic (severity from err)
      collect_lifecycle_warnings(ast)     → Diagnostic (Warning)
  → publish_diagnostics

.hover / .goto_definition / .document_symbol / .semantic_tokens_full
  → look up doc by URI
  → run the appropriate analyzer over (tokens, ast)
```

Every request runs over the *cached* tokens + AST from the most
recent edit. There's no incremental scan or parse — every edit
re-runs both passes on the full document. For UI code this is
trivially fast; the LSP doesn't need anything more sophisticated.

---

## Capabilities

```rust
ServerCapabilities {
    text_document_sync: INCREMENTAL,
    semantic_tokens_provider: SemanticTokensOptions { full: true, range: false, … },
    hover_provider: true,
    definition_provider: true,
    document_symbol_provider: true,
}
```

What's **not** provided:
- No `references_provider` / find-references.
- No `rename_provider`.
- No `completion_provider` / autocomplete.
- No `formatting_provider` / formatter.
- No `code_action_provider` / quick fixes.
- No `signature_help_provider`.
- No range-only semantic tokens.
- No incremental semantic tokens (`semanticTokens/full/delta`).

These are deliberate omissions. The server ships what's
load-bearing for `.ogh` editing today; missing features are
fair game.

---

## Tenets

- **The LSP depends on `scanner` and `parser` only. It never
  instantiates a `Runtime` or a widget tree.** This is the
  pipeline-stage seam from
  [INTENT §1](INTENT.md#1-the-pipeline-is-four-distinct-passes-not-three).
  Hover, goto-def, and semantic tokens all work over the AST.
  Running user code to provide tooling would be a security and
  correctness disaster (the file could have side effects;
  hovers shouldn't trigger them).

  *Why:* the four-pass pipeline is exactly designed to support
  this. The AST is a faithful structural view of the source;
  the LSP can answer most editor questions without going
  further. Anything that *would* need runtime data
  (live types from execution, host-state values, etc.) is a
  feature for a later, opt-in path — not the default.

  *Drift indicators:*
  - `use ogham::runtime::` in `src/lsp/`.
  - `use ogham::widget::` in `src/lsp/`.
  - LSP code that reads `.ogh` source from a file other than
    the buffer (would be a side-channel that bypasses the
    document store).

- **LSP positions are 0-indexed; Ogham spans are 1-indexed.**
  Conversion happens at the boundary in `server.rs`. The server
  adds 1 to incoming `(line, character)` before passing to
  analyzers; analyzers return `Span`s, which the server
  converts back via `goto_definition::span_to_range`.

  *Why:* both conventions are used widely (LSP follows JSON-RPC
  conventions; compilers tend to be 1-indexed for
  human-readable diagnostics). Picking one consistently for the
  internal AST and converting at the boundary is the simplest
  contract.

  *Drift indicators:*
  - An analyzer that takes 0-indexed positions.
  - A server that forgets to convert and silently returns
    off-by-one results.

- **Documents are cached as `(source, tokens, ast)` triples.**
  `Document::analyze` re-scans and re-parses the full source on
  every edit. Tokens are stored even when the AST fails to
  parse — that's how the `Error` token diagnostics (and
  semantic tokens for known token types) keep working through
  syntax errors.

  *Why:* the document holds *both* tokens and AST so the
  semantic tokens analyzer can work over tokens (for
  keyword/operator highlighting, which works regardless of
  parse success) *and* AST (for variable-vs-function
  classification, which requires parse success).

  *Drift indicators:*
  - A cache that drops tokens when parsing fails — would
    lose syntax highlighting on broken code.
  - A scanner-only LSP that can't classify identifiers
    semantically.

- **Diagnostics come from three sources.** Scanner `Error`
  tokens become diagnostics first; the parser's `SyntaxError`
  (including typed-bindings AST validation surfaced through
  `Document::parse_schema` as `doc.schema_error`) becomes one
  more diagnostic at its reported position; lifecycle-hook
  conditional-registration warnings (`collect_lifecycle_warnings`)
  add Warning-severity diagnostics for any `on_mount` /
  `on_unmount` / `effect` / `cleanup` that appears inside an
  `if`. There is no parser recovery — a parse error truncates
  AST-driven analysis at the error point (lifecycle warnings
  rely on a complete AST, so they only run when parse
  succeeds).

  *Why:* a "real" parser-recovery layer would let later
  errors surface even after an early one. Worth doing
  eventually; today the trade-off is simplicity vs. one
  diagnostic per save (which mostly works for short files).
  Lifecycle conditional-registration warnings are the only
  semantic check that flows through the LSP today; the
  schema-diagnostic engine (`src/diagnostics/`) is a separate
  consumer (`ogham check` CLI) and is not yet wired into the
  LSP — see open questions.

  *Drift indicators:*
  - A diagnostics path that doesn't include scanner Error
    tokens (regressing a basic feature).
  - Multiple parser errors per document without a recovery
    strategy — would mean the parser silently swallowed
    intermediate errors.
  - Lifecycle warnings firing on `on_mount` blocks at
    fn-body scope (the warning is specifically about
    `if`-gated registration; body-level conditionals inside
    the hook body are encouraged).

- **Edits are applied incrementally to `source`, then
  analyze re-runs the whole thing.** `apply_edits` walks
  `content_changes` and applies each via `replace_range` over
  byte offsets converted from UTF-16 code units. After all
  edits land, `analyze` re-scans + re-parses the whole text.

  *Why:* full re-analyze is fast enough for `.ogh` files. The
  byte/UTF-16 conversion (`byte_offset_of_utf16_cu`) is the
  load-bearing part for editors that send UTF-16 offsets (most
  do; LSP says they should).

  *Drift indicators:*
  - Treating LSP positions as byte offsets (would corrupt
    multi-byte source).
  - Skipping the byte-offset conversion when the source is
    "obviously ASCII" (one non-ASCII character later breaks
    everything).

- **Tower-LSP is the transport.** Stdio in/out via
  `tokio::io::stdin()` / `stdout()`. The server doesn't
  support TCP / Unix-socket transports. Most editors that talk
  LSP support stdio.

---

## Per-feature notes

### Hover

`hover::hover_at(ast, line, col) -> Option<HoverInfo>` walks
the AST looking for the deepest span containing `(line, col)`.
Returns markdown content describing the identifier:

- For a `let` binding: shows the name and (best-effort) the
  inferred shape from the assigned value.
- For a `state` declaration: shows the name as state.
- For function parameters: shows the parameter type identifier
  (advisory — types are unenforced).
- For widget identifiers: shows the widget type name.
- For lifecycle keywords (`on_mount`, `on_unmount`, `cleanup`):
  emits a `LifecycleHook { kind }` hover variant with a
  description of when the hook fires.
- For `effect (deps)`: emits an `Effect { dep_count }` hover
  variant noting the dependency count and re-fire semantics.

### Goto definition

`goto_definition::definition_at(ast, line, col) -> Option<Span>`.
Looks for the identifier under the cursor and walks upward
through enclosing scopes to find the binding. Returns a span
that the server converts to a `Location { uri, range }`.

Limitations:
- Cross-file goto (following an `import`) is not implemented —
  the analyzer only looks at the current document's AST.
- Shadowing is handled by the innermost-binding-wins rule.

### Document symbols

`document_symbols::document_symbols(ast) -> Vec<DocumentSymbol>`
emits a tree of symbols for the editor's outline view:
top-level `let` and `state` declarations, function bodies (with
nested declarations as children).

### Semantic tokens

`semantic_tokens::build_semantic_tokens(tokens, ast) ->
Vec<SemanticToken>`. Two-source classification:

1. **From the token stream**: keywords (`let`, `if`, …) and
   operators (`+`, `==`, `&&`, …) get flagged unconditionally.
2. **From the AST**: identifiers are walked and classified by
   role — function names, parameters, variables, properties.
   The walker tracks an `AstContext` that knows which names
   are bound as functions vs. state vs. plain locals.

The two sources are merged, sorted by (line, column), deduped,
and delta-encoded into the LSP wire format.

Token types: `KEYWORD, VARIABLE, FUNCTION, PARAMETER, TYPE,
PROPERTY, NUMBER, STRING, OPERATOR`.
Token modifiers: `DECLARATION, READONLY` (the latter for
`let`-bound non-state variables).

#### Tenets — semantic tokens

- **Tokens-only classification works without the AST.** When
  the AST fails to parse, the server still returns
  keyword/operator highlighting from the token stream.
  Editors look correct on broken code.

  *Drift indicators:*
  - A semantic token implementation that returns nothing
    when `ast` is `None`.

- **AST and token sources can both produce a token at the
  same `(line, col)`.** The dedupe step keeps only the
  *first* one in sort order; the AST walker is responsible
  for emitting more specific classifications (e.g. an
  identifier shouldn't appear in the token stream — only
  AST-derived tokens classify identifiers).

  *Drift indicators:*
  - Conflicts where the token stream emits an identifier-
    typed entry that gets de-prioritized by an AST entry.

---

## Building and running

```sh
cargo build --bin ogham-lsp        # build
./target/debug/ogham-lsp           # run (stdio transport)
```

There's a VS Code extension in `editors/vscode/`. Set
`ogham.lspPath` in the extension settings to the built binary
path; defaults to `"ogham-lsp"` on `$PATH`.

---

## Open questions (for the design-review phase)

- **No completion.** Authors writing widgets / properties /
  styles get no autocomplete. Property names are statically
  known per widget type; a low-effort completion provider
  could be very useful.
- **No find-references.** With AST + identifier tracking, this
  is mostly mechanical; the cost is wiring it up across the
  document store (and, eventually, across imports).
- **No cross-file analysis.** Goto-def, references, and
  workspace symbols all stop at the file boundary. A workspace
  index of all `.ogh` files would help; it would also need a
  watch mechanism so external edits invalidate the cache.
- **No formatter.** A canonical formatter would solve a lot of
  authoring grief (especially with widget literal indentation).
  Doable; needs a style decision.
- **Diagnostics from a single parse error.** Recovery would
  surface multiple errors per save. Tractable with a panic-mode
  parse-recovery design.
- **Hover for built-in widget properties** would surface
  property types and animation eligibility without the user
  reading the docs.
- **Type checking.** Today's `int`, `string`, `widget` annotations
  are advisory. The LSP could surface mismatches without
  enforcing them at the runtime layer — opt-in static checking
  on top of dynamic typing.
- **Schema-diagnostic engine integration.** The engine in
  `src/diagnostics/` validates a `.ogh` AST against a typed-
  bindings manifest emitted by `#[derive(OghamState)]` /
  `#[derive(OghamMsg)]` and powers the `ogham check` CLI. The
  LSP doesn't currently invoke it (no `use ogham::diagnostics`
  in `src/lsp/`). Wiring it through would let a `.ogh` file's
  references to `host_state` / `events` declarations surface
  drift against the host's current Rust types directly in the
  editor. See [SCHEMA_DIAGNOSTICS.md](SCHEMA_DIAGNOSTICS.md).
- **Performance: per-edit full re-parse.** Fine for typical UI
  files. A pathological 10k-line `.ogh` would re-scan +
  re-parse on every keystroke. Incremental parsing (tree-sitter
  shaped) is the long-term answer.
- **No support for the LSP's "code lens"** (showing inline
  hints over functions / state declarations). Could surface
  state declaration diagnostics inline (e.g. "this state isn't
  read anywhere" — though that requires usage tracking the LSP
  doesn't do today).
- **The LSP swallows tower-lsp shutdown semantics.**
  `shutdown` returns `Ok(())` immediately. If a future change
  needs to flush state, it'd need wiring up — easy fix, just
  noting.
