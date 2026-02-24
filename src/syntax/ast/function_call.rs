use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Symbol;
use super::args::Args;
use super::name::Name;
use super::prefix_exp::PrefixExp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FunctionCall {
    pub span: Span,
    pub prefix: Box<PrefixExp>,
    pub method: Option<Name>,
    pub args: Args,
}

impl Parsable for FunctionCall {
    /// Parses a fresh prefix and requires at least one call suffix.
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let prefix_exp = PrefixExp::parse(p)?;
        match prefix_exp.kind {
            super::prefix_exp::PrefixExpKind::Call(call) => Ok(call),
            _ => Err(ParseError {
                message: "expected function call".to_string(),
                position: prefix_exp.span.start,
            }),
        }
    }
}
