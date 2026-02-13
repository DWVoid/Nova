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

### 1.1.1 Example Output Style (Sketch)
```
Chunk {
  span: 0..42,
  leading_comments: [
    Comment { kind: Line, text: "-- hello", span: 0..8 },
  ],
  block: Block {
    stats: [
      Stat::LocalAssign {
        names: ["x"],
        exprs: [Exp::Number(1.0)],
        span: 9..18,
        trailing_comments: [],
      },
    ],
    ret: None,
  },
}
```

### 1.2 Comment Preservation Model
- Parse comments as tokens (trivia) and attach to AST nodes.
- Attachment rule (deterministic):
  - **Leading comments**: contiguous comments immediately preceding a node with no blank lines.
  - **Trailing comments**: comments on the same line after a node.
  - **Detached comments**: comments separated by blank lines; keep at block level.
- Represent comment spans: `{ kind: "line"|"block", text: String, span: {start,end} }`.

Deliverable: `docs/frontend/parser_plan.md` (this document) + AST format appendix.

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

### Step 4 — Parser (Statements and Blocks) (In Progress)
- Implement parser for chunk, block, and statement types.
- **Currently implemented**: empty statement (`;`), `return` (with/without exprs), assignments, local assignments, function declarations, control-flow (`if/elseif/else`, `while`, `repeat`, `for`), `do`, `break`, `goto`, and labels.
- **Comment attachment (partial)**: leading comments are split by blank lines (detached vs attached); trailing comments are attached at statement/return boundaries and propagated into expressions and prefix chains.
- **Rule added**: prefer trailing comments from the last expression when a statement wraps an expression list (return/assign/call).
- **Missing**: edge cases where both statement and expression could legitimately own trailing comments.
- Attach comments to AST nodes per rules.
- Unit tests: parse and round-trip test for statement types.

### Step 5 — Expression Parsing (In Progress)
- Implement expression parsing with precedence:
  - `or`, `and`, comparisons, bitwise ops, shifts, concatenation, arithmetic, unary, exponentiation.
  - Right/left associativity as defined in `3.4.8`.
- Expression parsing now includes prefix-expression suffixes (`.`, `[]`, call, method).
- **Currently implemented**: literals, unary/binary ops, tables, varargs (with function-body guard), function expressions, prefix expressions, calls/method calls, table fields.
- Unit tests: expression precedence and associativity, unary ops, concatenation.

### Step 6 — AST Emission (In Progress)
- Implement AST to text emission:
  - Pretty-print AST nodes with stable field ordering.
  - Manual string escaping for Lua syntax.
  - Include comment text and spans.
- Unit tests: emitted output matches golden files, comments and spans present.

## 3.1) Current Status (Checked Against Workspace)

- `src/token.rs` and `src/lexer.rs` are complete and tested.
- `src/ast.rs` added with Lua 5.4 AST node definitions and basic tests.
- Parser is split into modules under `src/parser/` with `src/parser/mod.rs` as the entry point.
- `src/parser/stat.rs` covers statements, blocks, and control flow parsing.
- `src/parser/expr.rs` covers expression parsing, table constructors, and function bodies.
- `src/parser/prefix.rs` covers prefix-expression suffixes and call arguments.
- `src/parser/helpers.rs` provides token helpers and precedence utilities.
- `src/parser/tests.rs` holds parser unit tests.
- `src/emit.rs` implements a structured, multi-line AST pretty-printer for all current AST nodes.
- `src/main.rs` wires stdin -> lexer -> parser -> emitter.

### Known Gaps
- Detached comments are only tracked at the block level; per-node detached behavior is still coarse.
- Some trailing comment precedence edge cases remain (multi-expression statements).
- Label syntax errors still rely on generic token expectations; more targeted diagnostics are possible.

### Next Steps
- Decide whether per-node detached comments are needed beyond block-level aggregation.
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

- AST pretty-printer should avoid trailing whitespace and align braces consistently.
- Consider a small `Printer` helper with explicit indentation control.
- Keep `Span` printable as `start..end` plus optional line/col if needed.
- Line breaks count only ASCII `\n`, `\r\n`, and `\r` sequences.
- Grapheme offsets are used for `Span` display and comparisons; byte offsets are retained for slicing.
- When emitting AST, prefer explicit `Span` formatting (`start..end`) and keep line/col optional.
- Consider a small `AstNodeId` counter only if needed for debugging; do not expose in output.
- AST nodes currently include comment vectors on most constructs; parser will decide exact attachment.
- `Stat` and `Exp` are wrapper structs to keep span/comments consistent for printing.
- Consider a separate `DetachedComments` list on `Block` for blank-line-separated trivia.
- Emitter currently prints structured AST nodes with stable ordering; prefer this over `Debug` output.
- Parser errors should include near-token context when available.
- Early parser support includes `return` without expressions; expression parsing will extend this.
- Block end detection currently keyed to `end`, `else`, `elseif`, `until`, and EOF.
- Prefix-expression parsing now supports chained field/index access and call/method suffixes.
- Comment attachment for suffix chains should prefer trailing comments on the last suffix token.
- Statement parsing will need careful ambiguity handling between assignment vs function call statements.
- Detached comments should be populated at block boundaries (blank-line separation).
- Local attributes (`<const>`, `<close>`) are parsed after each local name; consider dedicated diagnostics for invalid attributes.
- Labels are parsed as `::name::`; ensure comment attachment and span selection are well-defined.
- Statement parsing now covers assignment vs call disambiguation based on prefix-expression kind.
- Trailing comments are now attached to statements/returns via last-consumed token trivia.
- Detached comment handling still needs a dedicated pass in `parse_block`.
- Consider a helper to drain pending comment trivia to avoid accidental reuse.
- Added tests for detached comments, label comments, and local-attribute comments in `src/parser/tests.rs`.
- Multi-expression statements currently use the last expression to supply trailing comments.