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
keywords `let state if else return log fn for in match import
from`, the boolean literals `true false`, `Identifier(String)`
`String(String)` `Integer(i32)` `Float(f64)`, and the catch-all
`Error(String)` for unrecognized characters or unterminated
strings/comments.

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
| (anything else) | `ExpressionStatement` via `expression()` |

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

### AST node types

All `Statement` and `Expression` variants live under
`src/parser/`:

- `Statement::{Expression, Declare, DeclareState, Assign, Return,
  Conditional, Log, ForLoop, Import}` — see `statement.rs`.
- `Expression::{Literal, Unary, Binary, Grouping, Widget,
  MemberAccess, Call, IndexAccess, Range, ForLoop,
  SpreadForLoop, Spread, Match, PrefixIncrement,
  PostfixIncrement}` — see `expression.rs`.
- `Literal::{Integer, Float, Boolean, String, Identifier, Map,
  Array, Function}` — see `literal.rs`.

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
