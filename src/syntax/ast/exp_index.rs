use super::exp::Exp;
use crate::lexical::Span;
use serde::{Deserialize, Serialize};

/// Index access expression: `prefix[index]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpIndex {
    pub span: Span,
    pub prefix: Box<Exp>,
    pub index: Box<Exp>,
}

impl ExpIndex {
    pub fn new(span: Span, prefix: Exp, index: Exp) -> Exp {
        Exp::Index(ExpIndex {
            span,
            prefix: Box::new(prefix),
            index: Box::new(index),
        })
    }
}
