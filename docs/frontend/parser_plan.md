# Nova Lua 5.4 Parser Plan

> Scope: Implement a Lua 5.4-compliant parser in Rust using only the standard library, except for Unicode grapheme segmentation. Input is read from stdin, output is an AST with comments preserved. This plan splits work into steps with per-file/step size under 1000 LOC.

## 0) Sources and Spec Targets

- Primary spec: `docs/doc/manual.html`
- Key sections to map:
  - Lexical structure: `3.1`
  - Grammar and syntax: `3`, `3.3`, `3.4`
  - Operators/precedence: `3.4.8`
  - Long strings/comments: `3.1`
  - Examples and reference: `9`

## 1) Output Format and Comment Strategy (Design)

### 1.1 AST Output Format (Textual)
- **Rust representation**: print a Rust-shaped structure (struct/enum names and fields) rather than JSON.
- **Human-friendly, multi-line**: indented, one field per line, stable field ordering.
- Requirements:
  - Deterministic ordering of fields.
  - Manual string escaping (no external crates).
  - Include comment text and exact source span.
  - **Offsets**: spans use grapheme-cluster offsets; line counting uses ASCII line breaks.

### 1.2 Comment Preservation Model (Updated)
- **All comments are stored only on `Chunk`.**
- Parser collects every comment token (leading and trailing) into `Chunk.comments`.
- AST nodes below `Chunk` do not carry comment fields, so no attachment rules are applied.
- The order of comments is preserved as they appear in the token stream.

### 1.2.1 Example Output Style (Sketch)
```
Chunk {
  span: 0..42,
  comments: [
    Comment { kind: Line, text: "-- hello", span: 0..8 },
  ],
  block: Block {
    stats: [
      Stat::LocalAssign {
        names: ["x"],
        exprs: [Exp::Number(1.0)],
        span: 9..18,
      },
    ],
    ret: None,
  },
}
```

## 2) Rust Module Layout (Under 1000 LOC per file)

Target files (estimate):
- `src/token.rs` (≤ 400 LOC): token definitions, spans, comment trivia.
- `src/lexer.rs` (≤ 900 LOC): lexer for Lua 5.4.
- `src/ast.rs` (≤ 900 LOC): AST structs/enums + comment fields.
- `src/parser/mod.rs` (≤ 200 LOC): parser entry point, shared state, module wiring.
- `src/parser/expr.rs` (≤ 600 LOC): expression parsing, table constructors, function bodies.
- `src/parser/prefix.rs` (≤ 400 LOC): prefix-expression chains and call arguments.
- `src/parser/stat.rs` (≤ 900 LOC): statements, blocks, and control flow parsing.
- `src/parser/helpers.rs` (≤ 400 LOC): token helpers, operator precedence, block-end logic.
- `src/parser/tests.rs` (≤ 600 LOC): parser unit tests.
- `src/emit.rs` (≤ 600 LOC): AST to text output.
- `src/main.rs` (≤ 200 LOC): stdin -> lexer -> parser -> emit.
- `src/tests/` or inline `mod tests` (per module).

## 3) Stepwise Implementation Plan (Each Step ≤ 1000 LOC)

### Step 1 — Token and Span Types (Done)
- Implement `Position { byte, grapheme, line, column }` using Unicode grapheme clusters.
- Token enum for keywords, identifiers, literals, operators, punctuation, eof.
- Comment struct + trivia list; token carries leading/trailing trivia.
- Unit tests: grapheme advancement and span merge, token formatting.

### Step 2 — Lexer (Done)
- Implement Lua 5.4 lexical rules:
  - Identifiers, keywords, numerals, string literals.
  - Long strings/comments with `[[` or `[=[` levels.
  - Single-line (`--`) and long comments.
  - Operators and separators (per spec).
  - Line counting and spans.
- Store comments as trivia; do not drop.
- Unit tests: token streams for sample snippets in spec `3.1` and `9`.

### Step 3 — AST Definitions (Done)
- Define AST for chunks, blocks, statements, expressions, table constructors, function bodies.
- Include `leading_comments`, `trailing_comments`, and `span` per node.
- Keep Lua 5.4 grammar fidelity.
- Unit tests: AST construction helpers and debug output.

### Step 4 — Parser (Statements and Blocks) (Done)
- Implement parser for chunk, block, and statement types.
- All statements and control flow are implemented (see earlier progress).
- **Comment model updated**: parser no longer attaches comments to nodes.
- All comments are collected at `Chunk.comments`.

### Step 5 — Expression Parsing (Done)
- Expression parsing and prefix chains are complete.
- **Comment attachment removed** in expressions, tables, and prefix chains.

### Step 6 — AST Emission (Done)
- AST pretty-printer now prints `Chunk.comments` only.
- No per-node comment fields are emitted.

## 3.1) Current Status (Checked Against Workspace)

- `src/token.rs` and `src/lexer.rs` are complete and tested.
- `src/ast.rs` now stores comments only on `Chunk` (`Chunk.comments`).
- Parser is split into modules under `src/parser/` with `src/parser/mod.rs` as the entry point.
- `src/parser/stat.rs`, `src/parser/expr.rs`, and `src/parser/prefix.rs` no longer attach comments to nodes.
- `src/parser/helpers.rs` retains token helpers only; comment attachment helpers were removed.
- `src/parser/tests.rs` now asserts comments only on `Chunk`.
- `src/emit.rs` prints `Chunk.comments` only.
- `src/main.rs` wires stdin -> lexer -> parser -> emitter.

### Known Gaps
- Comment ordering is tied to token stream order (leading/trailing combined). If you want stable positional sorting, add a span-based sort step.
- Diagnostics still use generic error messages for label syntax issues.

### Next Steps
- Decide whether `Chunk.comments` should be sorted by span or preserve token order only.
- Add targeted diagnostics for label syntax (e.g., missing closing `::`).
- Clean up remaining warnings in `src/emit.rs` and `src/token.rs`.

## 4) Parser Strategy Details

### 4.1 Grammar Mapping
- Follow Lua 5.4 grammar from `manual.html`.
- `exp` uses precedence table:
  - `or`, `and`, comparisons, bitwise ops, shifts, concatenation, arithmetic, unary, exponentiation.
  - Right/left associativity as defined in `3.4.8`.

### 4.2 Error Recovery
- Minimal: stop at first error; report line, column, snippet.
- Optional later: synchronize at `;`, `end`, `until`, `elseif`, `else`.

## 5) Unit Test Plan

### 5.1 Lexer Tests
- Numeric literals (decimal, hex, scientific).
- Long strings with `=` levels.
- Comments in different positions.
- All operator tokens.

### 5.2 Parser Tests
- Statements: assignment, local, function, if/elseif/else, while, repeat, for, goto, label, break.
- Expressions: precedence, associativity, unary, concatenation.
- Tables: list, record, mixed.
- Functions: varargs, method calls, closures.
- Comments: ensure preserved in AST with correct attachment.

### 5.3 Output Tests
- Compare AST output string to golden files.
- Ensure comments and spans present.

## 6) Acceptance Criteria

- Parses Lua 5.4 samples from `docs/doc/manual.html`.
- Produces deterministic AST with comments preserved.
- Uses only standard library plus `unicode-segmentation` for graphemes.
- All tests pass in `cargo test`.

## Appendix A — Keywords and Tokens (From Spec)

- Keywords: `and break do else elseif end false for function goto if in local nil not or repeat return then true until while`.
- Operators and punctuation: `+ - * / % ^ # & ~ | << >> // == ~= <= >= < > = ( ) { } [ ] ; : , . .. ...`.

## Appendix B — Documentation Remarks (Non-code Ideas)

- Comment handling is now centralized: only `Chunk` carries comments.
- `Chunk.comments` is built by collecting leading + trailing comment trivia from all tokens.
- If you need per-node comments later, reintroduce attachment rules in parser modules.
- Span and grapheme rules remain unchanged.
- Emitters should only display the `Chunk.comments` list to avoid duplication.
- Tests should validate comments only at the `Chunk` level.