use super::ops::BinOp;
use crate::lexical::Span;
use serde::Serialize;

/// A binary operator expression: `a + b`, `a and b`, etc.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpBinary {
    pub span: Span,
    pub op: BinOp,
    pub left: Box<super::exp::Exp>,
    pub right: Box<super::exp::Exp>,
}
