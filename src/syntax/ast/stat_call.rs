use serde::Serialize;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::function_call::FunctionCall;
use super::prefix_exp::{PrefixExp, PrefixExpKind};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatCall {
    pub span: Span,
    pub call: FunctionCall,
}

impl Parsable for StatCall {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let prefix = PrefixExp::parse(p)?;
        match prefix.kind {
            PrefixExpKind::Call(call) => {
                if p.is_symbol(Symbol::Assign) || p.is_symbol(Symbol::Comma) {
                    return Err(ParseError {
                        message: "function call cannot be assignment target; use a variable or field"
                            .to_string(),
                        position: prefix.span.start,
                    });
                }
                Ok(StatCall { span: prefix.span, call })
            }
            _ => Err(ParseError {
                message: "expected function call".to_string(),
                position: prefix.span.start,
            }),
        }
    }
}
