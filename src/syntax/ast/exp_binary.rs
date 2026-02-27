use super::exp::Exp;
use super::ops::BinOp;
use crate::lexical::Span;
use serde::Serialize;

/// A binary operator expression: `a + b`, `a and b`, etc.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpBinary {
    pub span: Span,
    pub op: BinOp,
    pub left: Box<Exp>,
    pub right: Box<Exp>,
}

impl ExpBinary {
    pub fn new(span: Span, op: BinOp, left: Exp, right: Exp) -> Exp {
        Exp::Binary(ExpBinary {
            span,
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}
