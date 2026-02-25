use crate::lexical::Span;
use serde::Serialize;

/// Index access expression: `prefix[index]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpIndex {
    pub span: Span,
    pub prefix: Box<super::exp::Exp>,
    pub index: Box<super::exp::Exp>,
}
