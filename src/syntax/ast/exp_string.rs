use super::exp::Exp;
use crate::lexical::Span;
use serde::{Deserialize, Serialize};

/// A string literal expression: `"hello"`.
/// The value has all escape sequences already resolved to their final form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpString {
    pub span: Span,
    pub value: String,
}

impl ExpString {
    pub fn new(span: Span, value: String) -> Exp {
        Exp::String(ExpString { span, value })
    }
}
