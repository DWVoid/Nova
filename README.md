# Nova

Nova is an in-progress Lua 5.4-compatible parser. It reads Lua source from standard input, lexes/parses it, and prints a human-friendly AST that preserves comments. Unicode grapheme cluster offsets are tracked for spans, and ASCII line breaks are used for line/column counting.

## Status

- Lexer and tokens implemented.
- AST definitions and AST emitter added.
- Parser implemented (statements, expressions, and prefix chains).

## Documentation Structure

- `docs/doc/`: Lua 5.4 HTML reference (source material).
- `docs/frontend/`: Implementation plans and parser notes.
- `docs/language/0-Lexical.md`: Implemented lexical structure (English + BNF).
- `docs/language/1-Grammar.md`: Implemented grammar structure (English + BNF).

## Try It

```bash
cargo test
```

Parse a Lua file and print the AST:

```bash
cargo run < input.lua
```