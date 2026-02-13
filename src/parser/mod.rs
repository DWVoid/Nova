mod expr;
mod helpers;
mod prefix;
mod stat;
#[cfg(test)]
mod tests;

use crate::ast::{Chunk, Comments};
use crate::token::{Comment, Position, Token, TokenKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: Position,
}

pub struct Parser {
    tokens: Vec<Token>,
    index: usize,
    vararg_allowed: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            vararg_allowed: false,
        }
    }

    pub fn parse_chunk(mut self) -> Result<Chunk, ParseError> {
        let comments = collect_all_comments(&self.tokens);
        let block = self.parse_block(BlockEnd::Chunk)?;
        let eof = self.expect_eof()?;
        let span = block.span.merge(eof.span);
        Ok(Chunk {
            span,
            comments,
            block,
        })
    }
}

fn collect_all_comments(tokens: &[Token]) -> Comments {
    let mut out: Vec<Comment> = Vec::new();
    for token in tokens {
        out.extend(token.leading.iter().cloned());
        out.extend(token.trailing.iter().cloned());
    }
    if let Some(last) = tokens.last() {
        if let TokenKind::Eof = last.kind {
            out.extend(last.leading.iter().cloned());
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockEnd {
    Chunk,
    Nested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Assoc {
    Left,
    Right,
}