use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::exp::{Exp, ExpKind};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatCall {
    pub span: Span,
    /// The call expression. Always `ExpKind::Call { .. }`.
    pub call: Exp,
}

impl Parsable for StatCall {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let exp = Exp::parse(p)?;
        match exp.kind {
            ExpKind::Call { .. } => Ok(StatCall { span: exp.span, call: exp }),
            _ => Err(ParseError {
                message: "expected function call expression".to_string(),
                position: exp.span.start,
            }),
        }
    }
}