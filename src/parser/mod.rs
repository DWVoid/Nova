mod expr;
mod helpers;
mod prefix;
mod stat;
#[cfg(test)]
mod tests;

use crate::ast::Chunk;
use crate::token::{Position, Token};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: Position,
}

pub struct Parser {
    tokens: Vec<Token>,
    index: usize,
    vararg_allowed: bool,
    detached_stack: Vec<crate::ast::Comments>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            vararg_allowed: false,
            detached_stack: Vec::new(),
        }
    }

    pub fn parse_chunk(mut self) -> Result<Chunk, ParseError> {
        let block = self.parse_block(BlockEnd::Chunk)?;
        let leading = block.leading_comments.clone();
        let eof = self.expect_eof()?;
        let trailing = eof.leading;
        let span = block.span.merge(eof.span);
        Ok(Chunk {
            span,
            leading_comments: leading,
            block,
            trailing_comments: trailing,
        })
    }
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