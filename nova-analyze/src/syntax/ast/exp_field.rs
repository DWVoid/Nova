use super::exp::Exp;
use super::name::Name;
use crate::lexical::Span;
use serde::{Deserialize, Serialize};

/// Field access expression: `prefix.name`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpField {
    pub span: Span,
    pub prefix: Box<Exp>,
    pub name: Name,
}

impl ExpField {
    pub fn new(span: Span, prefix: Exp, name: Name) -> Exp {
        Exp::Field(ExpField {
            span,
            prefix: Box::new(prefix),
            name,
        })
    }
}
