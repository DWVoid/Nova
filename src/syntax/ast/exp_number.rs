use super::exp::Exp;
use crate::lexical::Span;
use serde::Serialize;

/// A numeric literal expression: `42`, `3.14`, etc.
/// The value is stored as the source text.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpNumber {
    pub span: Span,
    pub value: String,
}

impl ExpNumber {
    pub fn new(span: Span, value: String) -> Exp {
        Exp::Number(ExpNumber { span, value })
    }
}
