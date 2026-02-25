use super::exp::Exp;
use crate::lexical::Span;
use serde::Serialize;

/// The `nil` literal expression.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpNil {
    pub span: Span,
}

impl ExpNil {
    pub fn new(span: Span) -> Exp {
        Exp::Nil(ExpNil { span })
    }
}
