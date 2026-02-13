# Nova

Nova is an in-progress Lua 5.4-compatible parser. It reads Lua source from standard input, lexes/parses it, and prints a human-friendly AST that preserves comments. Unicode grapheme cluster offsets are tracked for spans, and ASCII line breaks are used for line/column counting.

## Status

- Lexer and tokens implemented.
- AST definitions and AST emitter added.
- Parser skeleton exists; statement/expression parsing is next.

## Try It

```bash
cargo test
```

When parsing is implemented, the CLI will support:

```bash
cargo run < input.lua
```
