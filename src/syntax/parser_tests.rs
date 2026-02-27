use super::parser::Parser;
use crate::lexical::lex;
use crate::syntax::ast::Chunk;

fn parse_chunk(input: &str) -> Chunk {
    let lex_result = lex(input).unwrap();
    Parser::new(lex_result.tokens, lex_result.trivia)
        .parse_chunk()
        .unwrap()
}

/// Full-chunk smoke test: namespace with no items is accepted and namespace
/// path is parsed correctly.  Individual item/expression parsing is covered by
/// the per-node unit tests in `src/syntax/ast/`.
#[test]
fn parses_minimal_compilation_unit() {
    let src = r#"
        namespace Example;
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.uses.len(), 0);
    assert_eq!(chunk.namespace.path.len(), 1);
    assert!(chunk.items.is_empty());
}

/// Verify that all comments in a source file are collected into `chunk.trivia`
/// in source order, regardless of their position relative to declarations.
#[test]
fn collects_comments_at_chunk_level() {
    let src = r#"
        -- leading
        namespace Example;
        -- trailing
    "#;
    let chunk = parse_chunk(src);
    let comment_count = chunk
        .trivia
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                crate::lexical::TriviaKind::LineComment | crate::lexical::TriviaKind::BlockComment
            )
        })
        .count();
    assert_eq!(comment_count, 2);
}