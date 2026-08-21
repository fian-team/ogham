# Ogham — Language Front-End (Scanner + Parser)

> **Status: Live contract.**
>
> The grammar, scanner, and AST shape — i.e. everything between
> source text and the bytecode compiler. The compiler / VM live in
> [`VM.md`](VM.md); the language *reference* (user-facing) lives
> in `docs/language/` once it exists. This doc is for contributors
> changing or reading the front end.

---

## At a glance

```
Source (UTF-8 .ogh)
  → Scanner          src/scanner/
      Vec<Token>     (one-shot lex; comments stripped; errors as tokens)
  → Parser           src/parser/
      Function       (top-level "module" wrapping a Block of Statements)
```

Both passes are single-shot: the scanner walks the input string
once and returns a `Vec<Token>`; the parser consumes that vec and
returns a `Function`. There is no streaming/incremental mode (the
LSP just re-runs both on every edit).

**Authority:**
- Scanner: [`src/scanner/mod.rs`](../../src/scanner/mod.rs),
  [`src/scanner/token.rs`](../../src/scanner/token.rs),
  [`src/scanner/token_type.rs`](../../src/scanner/token_type.rs).
- Parser entry: [`src/parser/mod.rs`](../../src/parser/mod.rs).
- AST nodes: the rest of `src/parser/`.

---

## Scanner

### What it produces

A `Vec<Token>` terminated by `TokenType::EOF`. Each token carries
`token_type`, `line` (1-indexed), `column` (1-indexed), `start`
(0-indexed byte offset into the input), and `length`.

The token enum (`TokenType` in `token_type.rs`) covers
punctuation, arithmetic, comparison, logical operators, the
core keywords `let state if else return log fn for in match
import from`, the typed-bindings keywords `record host_state
events Self` (Phase 1), the lifecycle keywords `on_mount
on_unmount effect cleanup` (Phase 2), the boolean literals
`true false`, `Identifier(String)` `String(String)`
`Integer(i32)` `Float(f64)`, and the catch-all `Error(String)`
for unrecognized characters or unterminated strings/comments.

### Tenets

- **Errors are tokens, not panics or `Result`s.** Unrecognized
  characters and unterminated strings/comments emit a
  `TokenType::Error(String)`. Scanning continues past the error.

  *Why:* the LSP needs to surface diagnostics for partial source
  on every keystroke. A `Result`-based scanner would block
  downstream analysis on the first bad character; an `Error`
  token lets the rest of the document still produce diagnostics.
  See `lsp/server.rs::collect_diagnostics`.

  *Drift indicators:*
  - `panic!` / `unwrap` in scanner code paths reachable from
    user input.
  - A scanner change that bails on first error.
  - The scanner returning `Result<Vec<Token>, ...>` instead of
    `Vec<Token>`.

- **Comments are consumed in the scanner; the parser never sees
  them.** Both `// line` and `/* block */` are eaten;
  unterminated block comments emit an `Error` token but don't
  stop scanning.

  *Why:* the parser already has enough mode-state. Letting it
  peek over comments would mean every `next_is` / `peek` would
  need a comment-skip wrapper, and comment positions inside
  expressions are a known nuisance source for ASIs/precedence.

  *Drift indicators:*
  - A `TokenType::Comment` variant.
  - Parser code that asks "is this a comment?".

- **`..` and `...` resolve in the scanner, not the parser.**
  Three dots is `Spread`; two dots is `Range`; a number followed
  by `..` keeps the dots as a separate token (so `0..10` scans
  as `Integer(0) Range Integer(10)`, not `Float(0.) ...`). The
  number-then-`..` handling in `consume_number` is load-bearing
  for `for (i in 0..10)` working naturally.

  *Drift indicators:*
  - A scanner change that produces `Float(0.)` for the first
    half of `0..10`.
  - A new token type that conflicts with the `.. / ...` lookahead.

- **Identifiers are alphanumeric+underscore, starting with a
  letter or underscore.** Keywords are looked up by string match
  after the identifier loop completes — so `letter` is an
  identifier, not `let` followed by `ter`.

### What's not handled

- **No string interpolation** (no `${expr}` inside `"..."`).
- **No string escapes.** Backslashes in strings are passed
  through verbatim. `consume_string` reads until the next `"`
  with no escape handling.
- **No numeric prefixes.** `0x`, `0b`, `0o` are not understood;
  underscores in numeric literals (`1_000`) are not handled.
- **No raw-string syntax.**

These are deliberate omissions, not unintended drift. Any of
them is fair game for the design-review phase.

---

## Parser

### Top-level shape

`Parser::parse` calls `parse_block(allow_import=true)` and wraps
the result in a `Function` named `"<module>"`. `parse_block`
reads statements until it hits `EOF` or an unmatched `}`.

The parser's only piece of mode-state is `parsing_match_scrutinee:
bool`. It exists to disambiguate `Identifier {` — which is
ordinarily a widget literal but, immediately after `match`, must
be parsed as an identifier (the scrutinee) followed by the
match-arms block.

### Statement grammar

Statements are dispatched by the leading token in
`parse_statement`:

| Leading token | Statement type                            |
|---------------|-------------------------------------------|
| `import`      | `ImportStatement` (top-level only)        |
| `if`          | `ConditionalStatement` (chains else if/else) |
| `return`      | `ReturnStatement` (with optional value)   |
| `let`         | `DeclareStatement`                        |
| `state`       | `DeclareStateStatement`                   |
| `Identifier`  | `parse_identifier_statement` — see below  |
| `log`         | `LogStatement`                            |
| `for`         | `ForLoopStatement`                        |
| `record`      | `RecordDeclaration` (top-level only, Phase 1) |
| `host_state`  | `HostStateDeclaration` (top-level only, ≤1 per module) |
| `events`      | `EventsDeclaration` (top-level only, ≤1 per module) |
| `screen`      | `ScreenDeclaration` (top-level only, ids unique) — see below |
| `on_mount`    | `OnMount` lifecycle hook (fn-body only, Phase 2)   |
| `on_unmount`  | `OnUnmount` lifecycle hook (fn-body only, Phase 2) |
| `effect`      | `Effect` lifecycle hook (fn-body only, Phase 2)    |
| `cleanup`     | `Cleanup` (effect-body only, Phase 2)              |
| (anything else) | `ExpressionStatement` via `expression()` |

### `screen` — routable surfaces

```
screen "world" {
  state { rows: array<Row>, composing: string = "" }
  view world_panel()
};
```

A `screen` declares **one routable surface**: an id, the slice of host
state that surface alone reads, and the view it renders. It is the ogham
half of `lorekeeper/docs/ROUTING.md`; the rules that matter here:

- **The id is a route id, not an identifier.** `"map-edit"` is legal and
  would scan as three tokens if screens were named after it — so the
  compiler names each screen's closure `__ogh_screen_<index>` by its
  position in the (sorted) schema map.
- **`state` is optional; `view` is not.** A screen with no slice reads
  only the root scope, which is common. A screen with no view is an
  error naming the screen.
- **Neither `screen` nor `view` is a keyword.** Both are recognized
  contextually: `screen` only at module top level with a string literal
  following it, `view` only inside a screen body. `state` needed no
  decision — it was a keyword before any of this.

  This is not caution, it is a measurement. `screen` *was* a keyword for
  about an hour, and it broke three shipped documents across the repos
  on the first full test run: celia has a `screen(width, children)`
  layout helper, regency a `screen` host-state field. Both failed as
  `Expected identifier`, pointing at lines that had not changed in
  months — the same silent-at-a-distance shape as `import` and its
  friends being unusable as `host_state` keys. **A new keyword in this
  language costs every document in every repo that already used the
  word, and the error never says so.** Prefer a contextual form whenever
  one exists; here the declaration is narrow enough that no other use of
  either name can be mistaken for it.
- **Scoping is own-slice-then-root.** A screen's field compiles to the
  host-state key `"<id>::<field>"`, so two screens may both declare
  `rows` and neither can name the other's — a name a screen did not
  declare falls through to `host_state {}` as it always did.
- **`outlet` renders the injected path.** The host sets
  `__route_path` (an array of ids, outermost first) through
  `Runtime::set_route_path`, and `outlet()` renders those screens in
  order. It is forward-declared before the module body and assigned
  after it, because `main` is written last and the dispatcher it calls
  can only be built once every screen's closure exists — a module-level
  slot stays an open upvalue for the whole module frame, so `main` reads
  the real dispatcher when it finally runs.
- **A document never navigates.** There is no way to *set* the path from
  inside ogham, and that is `INTENT §10` holding rather than an
  omission.

Screen fields are seeded with their declared default (or an empty value
of the declared type) when the module is set, so a route that mounts and
immediately draws cannot fail on a field the host has not pushed yet.

`parse_identifier_statement` peeks one token to decide:
- `Identifier (` → call → expression statement
- `Identifier {` → widget literal → expression statement
- `Identifier =` → assignment statement
- otherwise → bare identifier with optional postfix
  (`.prop`, `[idx]`) → expression statement

### Expression grammar

Recursive-descent, classic precedence climb, **lowest precedence
first**:

```
expression       = logical_or
logical_or       = logical_and  ( "||" logical_and )*
logical_and      = equality     ( "&&" equality )*
equality         = comparison   ( ("=="|"!=") comparison )*
comparison       = term         ( ("<"|"<="|">"|">=") term )*
term             = range        ( ("+"|"-") range )*
range            = factor       ( ".." factor )?      // produces RangeExpression
factor           = exponent     ( ("*"|"/"|"%") exponent )*
exponent         = unary        ( "^" exponent )?      // right-associative
unary            = "++" Identifier             // prefix increment
                 | "-" unary
                 | "!" unary
                 | "..." for_loop_expression  // spread + for is a unit
                 | primary
primary          = Integer | Float | Boolean | String
                 | Identifier ( widget | bare-ident )?
                 | map-literal
                 | array-literal
                 | function-literal
                 | match-expression
                 | for-loop-expression
                 | "(" expression ")"

postfix          = ( "." Identifier
                   | "(" args ")"
                   | "[" expression "]"
                   | "++"                       // postfix increment
                   )*
```

After `primary`, `parse_postfix` chains member access, calls, and
index access in any combination — `arr[0].length()` parses as
intended.

### Tenets

- **Implicit return is a property of the parser, not the
  grammar.** The "trailing expression without semicolon =
  return" rule is folded into `expression_to_statement`: if the
  expression is followed by `;`, it's an `ExpressionStatement`;
  if it's at end-of-block (next token is `}` or `EOF`), it's
  rewritten as a `ReturnStatement`. This is invisible to the
  expression grammar.

  *Why:* keeping the rule out of the grammar means the
  recursive-descent precedence ladder doesn't have to reason
  about end-of-block. The compiler also benefits — see
  `compile_expression_block` in `compiler.rs`, which strips the
  return wrapping for inline expression evaluation (e.g. match
  arm bodies).

  *Drift indicators:*
  - A grammar change that adds `Return` as a producer of
    `expression`.
  - A compiler that synthesises a `Return` instead of relying on
    the parser-emitted one.

- **The widget / map / match-block ambiguities live in
  `primary` + `parsing_match_scrutinee`.** Three constructs use
  `{ ... }`: map literals, widget literals, and match arms.
  - **Map literal**: `primary` enters via the `LeftBracket` arm.
  - **Widget literal**: `primary` enters via `Identifier` and
    looks at the *next* token — if it's `{`, it's a widget,
    *unless* `parsing_match_scrutinee` is set.
  - **Match arms**: `parse_match_expression` sets the flag, parses
    the scrutinee, clears the flag, then parses `{ pat => body,
    ... }`.

  *Why:* without the flag, `match foo { 1 => "one" }` would
  parse `foo` as a widget literal opening a `{` block.

  *Drift indicators:*
  - A new construct that introduces `Identifier {` semantics
    different from a widget literal (without a corresponding
    flag).
  - Removing `parsing_match_scrutinee` and trying to handle the
    ambiguity by lookahead — easy to think you've got it, easy
    to be wrong.

- **Imports are a top-level-only statement.** `parse_block`
  takes an `allow_import: bool`; it's `true` only at the module
  body and `false` everywhere else. A nested `import` is a
  syntax error.

  *Why:* import resolution lifts top-level bindings into the
  module's environment. Imports inside functions would either
  pollute outer scope (surprising) or be invisible (pointless).
  Forbidding them is the simplest contract.

  *Drift indicators:*
  - A new caller of `parse_block(true)` outside the module
    entry.
  - Compiler code that handles imports nested inside functions
    (today's `compile_import_stmt` always runs at module scope).

### What crosses an import

```
import "./stationery.ogh";                  // everything it declares
import { plate, rule } from "./stationery.ogh";   // only these
```

Two things cross, and they arrive by two different routes:

| declaration | how it arrives | who reads it |
|---|---|---|
| `let name = …` | copied into the module environment at execution, and injected as host state so `GetState` finds it | the VM, and strict-mode identifier resolution |
| `record Foo { … }` | merged into the importing module's `ModuleSchema::imports` | the schema resolver, and every `Foo` a field is declared at |

`host_state {}` does **not** cross. It is a whole-document
declaration bound to one mount, and a module that carries one is
still strict on its own terms.

**The graph is walked transitively, and the walk mirrors
execution.** `runtime::imports::walk` is the one answer, read by
the compiler (which resolves identifiers), by the schema loader
(which resolves records), and by the watcher (which watches every
file the graph reached). Transitive because *execution* is: an
imported module runs inside the importing runtime, so a module two
hops away has already copied its names into the shared environment
by the time the document's own body runs. A compiler that only
pre-scanned direct imports would reject a helper that then ran
perfectly.

The one asymmetry is narrowing, and it is execution's too:
`import { a } from "x.ogh"` narrows *x's own* declarations to `a`,
and whatever `x` imported arrives beside it unnarrowed. The walk
reproduces that rather than the tidier rule, because two answers
is the bug.

**Resolution order** is embedded source (keyed by the import string
exactly as written) → prefix mapping → project root, with a missing
`.ogh` extension supplied. `ImportSpace` is that policy written
down once; a standalone schema load with no host configuration
roots it at the document's own directory.

**A hot edit re-derives the watch set.** The files a document is
made of are a *reading* of its import graph, not a fact about the
mount, so `Ogham::reload` rebuilds the watcher from the runtime
that just took over. An edit that adds an import starts watching
the module it added; an edit to any module reloads every document
that mounts it.

  *Drift indicators:*
  - A second import-resolution path that does not go through
    `ImportSpace::resolve`.
  - A reader of a mounted document's schema calling
    `ModuleSchema::from_module` (which knows nothing of imports)
    rather than `Runtime::module_schema`.

- **Type annotations are recorded but unenforced.** Function
  parameters require a type identifier (`fn (x: int)`); `let`
  and `state` declarations may optionally have one (`let x: int
  = 5`). Type identifiers can be array-suffixed (`int[]`,
  `widget[][]`). Nothing in the runtime checks them.

  *Why:* annotations exist for the LSP and for human readers,
  not for type safety. The runtime is dynamically typed; the
  prelude relies on `int` annotations on parameters that
  actually accept floats and ints (`rgb`, `rgba`).

  *Drift indicators:*
  - VM code that reads parameter types and rejects mismatches.
  - LSP code that *requires* a type annotation to provide
    tooling — annotations should remain advisory.

- **Lifecycle hook bodies parse with `in_effect_body` mode
  state.** `parse_effect` flips a flag while descending into
  the effect's body so `parse_cleanup` can refuse a `cleanup`
  outside an effect. The same flag is *not* set inside
  `parse_mount` / `parse_unmount`, so a `cleanup` inside an
  `on_mount` is a parse error.

  *Why:* `cleanup { ... }` is meaningful only as the back-edge
  of an `effect`'s body. Parser-side rejection gives the LSP a
  clean diagnostic at the call site; the alternative would be a
  compiler error that would have to be plumbed back through
  positional info.

  *Drift indicators:*
  - A new lifecycle keyword with cleanup-style semantics that
    forgets to set/check `in_effect_body`.
  - A change that allows `cleanup` inside `on_mount` /
    `on_unmount` (the runtime has no slot to attach it to).

### AST node types

All `Statement` and `Expression` variants live under
`src/parser/`:

- `Statement::{Expression, Declare, DeclareState, Assign, Return,
  Conditional, Log, ForLoop, Import, RecordDeclaration,
  HostStateDeclaration, EventsDeclaration, OnMount, OnUnmount,
  Effect, Cleanup}` — see `statement.rs`. The last seven were
  added in Phase 1 (typed bindings) and Phase 2 (lifecycle
  hooks).
- `Expression::{Literal, Unary, Binary, Grouping, Widget,
  MemberAccess, Call, IndexAccess, Range, ForLoop,
  SpreadForLoop, Spread, Match, PrefixIncrement,
  PostfixIncrement}` — see `expression.rs`.
- `Literal::{Integer, Float, Boolean, String, Identifier, Map,
  Array, Function}` — see `literal.rs`.
- Phase 1 typed-bindings types (`RecordDecl`, `HostStateDecl`,
  `EventsDecl`, `RecordField`, `EventVariant`) live in
  `typed_bindings.rs`.

Every node carries a `Span` (start/end line+column). The compiler
uses `span.start_line` for emitted-bytecode line numbers.

### Tests

Both passes have inline `#[cfg(test)]` modules; the parser tests
in `parser/mod.rs` cover let/fn/if/state/widget/match/for/import/
array/precedence cases. They're the de facto language acceptance
tests.

---

## Open questions (for the design-review phase)

- **Why `^` for power instead of `**`?** Most languages use `**`
  (Python, JS, Rust's `pow`); `^` traditionally means XOR.
  Conflict with future bitwise ops.
- **Why does identifier-starting-with-`{` always default to
  widget?** Pretty-printed map literals on the right of
  `let x = Identifier { … }` could plausibly want to be a map
  with `Identifier` as a label rather than a widget call. Not a
  current ambiguity (no labels exist) but worth interrogating.
- **No string escapes.** Authors who need a `"` in a string have
  no recourse. Cheap to add; deliberately omitted so far.
- **Type annotations as advisory comments.** They have a syntax
  cost (fn params require them) without a behavioral cost. Either
  enforce them or make them syntactically optional everywhere.
- **`for` only supports integer ranges.** `for (i in 0..10)` is
  the only iteration shape. No `for (item in array)`. Authors
  reach for `for (i in 0..arr.length())` plus `arr[i]`.
- **`match` patterns are literals or identifiers only.** No
  destructuring, no guards, no constructor patterns.
- **Identifier statements with three different meanings is a
  smell.** `parse_identifier_statement` distinguishes call,
  widget literal, and assignment by single-token lookahead. If
  an `Identifier { ... }` ever needs a different meaning, the
  whole shape needs revisiting.

### `for` in an array literal does not splice

```
children: for (i in 0..rows.length()) { row(rows[i]) }   // n children
children: [ header(), for (i in 0..rows.length()) { … } ] // ONE child
```

The first form is the whole `children` value and expands to one child per
iteration. The second puts the loop *inside* an array literal, where it is
a single element — and one that renders as nothing, so a list simply does
not appear while everything around it does.

Nothing warns. Ashworth Manor's pause overlay drew its heading and none of
its rows for exactly this reason, and the route projecting them was
verified correct first. Put the loop in its own container when it needs
siblings.
