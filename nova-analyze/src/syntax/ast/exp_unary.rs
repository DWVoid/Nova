use super::exp::Exp;
use super::ops::UnOp;
use crate::lexical::Span;
use serde::{Deserialize, Serialize};

/// A unary operator expression: `-x`, `not x`, `#x`, `~x`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpUnary {
    pub span: Span,
    pub op: UnOp,
    pub exp: Box<Exp>,
}

impl ExpUnary {
    pub fn new(span: Span, op: UnOp, exp: Exp) -> Exp {
        Exp::Unary(ExpUnary {
            span,
            op,
            exp: Box::new(exp),
        })
    }
}
