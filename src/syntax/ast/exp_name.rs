use super::exp::Exp;
use crate::lexical::Span;
use serde::{Deserialize, Serialize};

/// A bare identifier expression: `foo`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpName {
    pub span: Span,
    pub name: String,
}

impl ExpName {
    pub fn new(span: Span, name: String) -> Exp {
        Exp::Name(ExpName { span, name })
    }
}
