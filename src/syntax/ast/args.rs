use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::{Symbol, TokenKind};
use super::exp::Exp;
use super::initializer::Initializer;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ArgsKind {
    ExpList(Vec<Exp>),
    Initializer(Initializer),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Args {
    pub span: Span,
    pub kind: ArgsKind,
}

impl Parsable for Args {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Symbol(Symbol::LParen) => {
                let open = p.advance();
                let mut exprs = Vec::new();
                if !p.is_symbol(Symbol::RParen) {
                    exprs = p.parse_exp_list()?;
                }
                let close = p.expect_symbol(Symbol::RParen)?;
                Ok(Args {
                    span: open.span.merge(close.span),
                    kind: ArgsKind::ExpList(exprs),
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let init = Initializer::parse(p)?;
                Ok(Args {
                    span: init.span,
                    kind: ArgsKind::Initializer(init),
                })
            }
            _ => Err(ParseError {
                message: "expected call arguments".to_string(),
                position: token.span.start,
            }),
        }
    }
}
