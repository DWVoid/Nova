use crate::syntax::ast::{Chunk, TopItem, Trivia, UseDecl, NamespaceDecl};
use crate::lexical::{Keyword, Position, Token, TokenKind, Trivia as LexTrivia};
use super::parsable::Parsable;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: Position,
}

pub struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) index: usize,
    pub(crate) trivia: Trivia,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, trivia: Vec<LexTrivia>) -> Self {
        Self { tokens, index: 0, trivia }
    }

    pub fn parse_chunk(mut self) -> Result<Chunk, ParseError> {
        let trivia = std::mem::take(&mut self.trivia);

        let mut uses = Vec::new();
        while self.is_keyword(Keyword::Use) {
            uses.push(UseDecl::parse(&mut self)?);
        }

        let namespace = NamespaceDecl::parse(&mut self)?;

        let mut items = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            items.push(TopItem::parse(&mut self)?);
        }

        let eof = self.expect_eof()?;
        let span = if let Some(last) = items.last() {
            let last_span = match last {
                TopItem::Definition(def) => def.span,
                TopItem::Implementation(imp) => imp.span,
            };
            namespace.span.merge(last_span).merge(eof.span)
        } else if let Some(last_use) = uses.last() {
            namespace.span.merge(last_use.span).merge(eof.span)
        } else {
            namespace.span.merge(eof.span)
        };
        Ok(Chunk { span, trivia, uses, namespace, items })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Assoc {
    Left,
    Right,
}
