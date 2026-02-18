# Nova

Nova is a modern programming language compiler built in Rust. Starting as a Lua 5.4-compatible parser, Nova is evolving into a complete language with its own syntax, semantic analysis, and compilation capabilities.

## Status

**Completed**:
- ✅ **Lexical Analysis**: Complete lexer with Unicode grapheme cluster support and comprehensive token types
- ✅ **Syntax Analysis**: Full parser implementation (statements, expressions, declarations, and complex constructs)
- ✅ **AST Generation**: Rich abstract syntax tree with source location preservation and comment attachment
- ✅ **Semantic Analysis Foundation**: Bundle system, dependency management, and semantic model structure
- ✅ **Integration Pipeline**: End-to-end compilation pipeline from source to semantic model

**In Progress**:
- 🚧 **Semantic Analysis**: Namespace resolution, symbol tables, and type checking
- 🚧 **Type System**: Type inference, constraint solving, and trait coherence
- 🚧 **Cross-Bundle Linking**: Symbol resolution across module boundaries

## Architecture

- **`src/lexical/`**: Tokenization and lexical analysis
- **`src/syntax/`**: Parser, AST definitions, and syntax tree emission
- **`src/semantic/`**: Semantic analysis, type checking, and bundle management
- **`bundle_manifest.rs`**: Bundle configuration and metadata handling

## Documentation Structure

- **`docs/language/`**: Language specification and grammar
  - `0-Lexical.md`: Lexical structure and tokenization rules
  - `1-Syntax.md`: Grammar specification and syntax rules  
  - `2-Semantic.md`: Semantic model and type system specification
- **`docs/plans/`**: Implementation plans and development roadmap
  - `0-syntax-plan.md`: Parser implementation plan (completed)
  - `1-semantic-plan.md`: Semantic analysis implementation plan (in progress)
- **`docs/frontend/`**: Implementation notes and design decisions

## Try It

Run all tests including semantic analysis:

```bash
cargo test
```

Parse and analyze Nova source:

```bash
echo "namespace Example; export define main(): unit end" | cargo run
```

The compiler will show both the parsed AST and semantic analysis results.