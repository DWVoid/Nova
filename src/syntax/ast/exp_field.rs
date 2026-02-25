use super::exp::Exp;
use super::name::Name;
use crate::lexical::Span;
use serde::Serialize;

/// Field access expression: `prefix.name`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpField {
    pub span: Span,
    pub prefix: Box<super::exp::Exp>,
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
