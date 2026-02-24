use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Symbol;
use super::exp::Exp;
use super::name::Name;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum FieldKey {
    Exp(Exp),
    Name(Name),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Field {
    pub span: Span,
    pub key: Option<FieldKey>,
    pub value: Exp,
}

impl Parsable for Field {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        use crate::lexical::TokenKind;
        if p.is_symbol(Symbol::LBracket) {
            let open = p.advance();
            let key = Exp::parse(p)?;
            p.expect_symbol(Symbol::RBracket)?;
            p.expect_symbol(Symbol::Assign)?;
            let value = Exp::parse(p)?;
            let span = open.span.merge(value.span);
            return Ok(Field {
                span,
                key: Some(FieldKey::Exp(key)),
                value,
            });
        }
        if let TokenKind::Identifier(_) = p.current().kind {
            if p.peek_is_symbol(1, Symbol::Assign) {
                let name = Name::parse(p)?;
                p.expect_symbol(Symbol::Assign)?;
                let value = Exp::parse(p)?;
                let span = name.span.merge(value.span);
                return Ok(Field {
                    span,
                    key: Some(FieldKey::Name(name)),
                    value,
                });
            }
        }
        let value = Exp::parse(p)?;
        Ok(Field {
            span: value.span,
            key: None,
            value,
        })
    }
}
