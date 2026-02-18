mod expr;
mod helpers;
mod prefix;
mod stat;
mod decl;
#[cfg(test)]
mod tests;

use crate::syntax::ast::{Chunk, Comments, NamespaceDecl, TopItem, UseDecl};
use crate::lexical::token::{Comment, Keyword, Position, Symbol, Token, TokenKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: Position,
}

pub struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    pub fn parse_chunk(mut self) -> Result<Chunk, ParseError> {
        let comments = collect_all_comments(&self.tokens);
        let uses = self.parse_use_decls()?;
        let namespace = self.parse_namespace_decl()?;
        let items = self.parse_top_items()?;
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
        Ok(Chunk {
            span,
            comments,
            uses,
            namespace,
            items,
        })
    }
}

fn collect_all_comments(tokens: &[Token]) -> Comments {
    let mut out: Vec<Comment> = Vec::new();
    for token in tokens {
        out.extend(token.leading.iter().cloned());
        out.extend(token.trailing.iter().cloned());
    }
    out
}

impl Parser {
    fn parse_use_decls(&mut self) -> Result<Vec<UseDecl>, ParseError> {
        let mut uses = Vec::new();
        while self.is_keyword(Keyword::Use) {
            uses.push(self.parse_use_decl()?);
        }
        Ok(uses)
    }

    fn parse_top_items(&mut self) -> Result<Vec<TopItem>, ParseError> {
        let mut items = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            items.push(self.parse_top_item()?);
        }
        Ok(items)
    }

    fn parse_top_item(&mut self) -> Result<TopItem, ParseError> {
        if self.is_keyword(Keyword::Define) || self.is_symbol(Symbol::At) || self.is_keyword(Keyword::Export) {
            return Ok(TopItem::Definition(self.parse_definition()?));
        }
        if self.is_keyword(Keyword::Implement) {
            return Ok(TopItem::Implementation(self.parse_implementation()?));
        }
        Err(ParseError {
            message: "expected top-level definition or implementation".to_string(),
            position: self.current().span.start,
        })
    }

    fn parse_namespace_decl(&mut self) -> Result<NamespaceDecl, ParseError> {
        let token = self.expect_keyword(Keyword::Namespace)?;
        let path = self.parse_namespace_path()?;
        let semi = self.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(NamespaceDecl { span, path })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum BlockEnd {
    Chunk,
    Nested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Assoc {
    Left,
    Right,
}