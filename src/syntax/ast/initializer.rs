use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Symbol;
use super::field::Field;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Initializer {
    pub span: Span,
    pub fields: Vec<Field>,
}

impl Parsable for Initializer {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let open = p.expect_symbol(Symbol::LBrace)?;
        let mut fields = Vec::new();
        while !p.is_symbol(Symbol::RBrace) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            fields.push(Field::parse(p)?);
            if p.is_symbol(Symbol::Comma) || p.is_symbol(Symbol::Semi) {
                p.advance();
            } else {
                break;
            }
        }
        let close = p.expect_symbol(Symbol::RBrace)?;
        let span = open.span.merge(close.span);
        Ok(Initializer { span, fields })
    }
}
