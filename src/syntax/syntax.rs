use crate::lexical::{Position, Token, Trivia};
use crate::lexical::LexicalResult;
use crate::syntax::ast::Chunk;
use crate::syntax::parse::Parser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxError {
    pub message: String,
    pub position: Position,
}

#[derive(Clone, Debug)]
pub struct SyntaxResult {
    pub chunk: Chunk,
}

pub fn transform(lex: LexicalResult) -> Result<SyntaxResult, SyntaxError> {
    let parser = Parser::new(lex.tokens, lex.trivia);
    Ok(SyntaxResult {
        chunk: parser.parse_chunk()?,
    })
}