use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::TokenKind;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Name {
    pub value: String,
    pub span: Span,
}

impl Parsable for Name {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.current().clone();
        if let TokenKind::Identifier(value) = token.kind {
            p.advance();
            return Ok(Name {
                value,
                span: token.span,
            });
        }
        Err(ParseError {
            message: "expected identifier".to_string(),
            position: token.span.start,
        })
    }
}
