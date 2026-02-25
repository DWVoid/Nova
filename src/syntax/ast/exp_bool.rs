use super::exp::Exp;
use crate::lexical::Span;
use serde::Serialize;

/// A boolean literal expression: `true` or `false`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpBool {
    pub span: Span,
    pub value: bool,
}

impl ExpBool {
    pub fn new(span: Span, value: bool) -> Exp {
        Exp::Bool(ExpBool { span, value })
    }
}
