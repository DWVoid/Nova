use crate::lexical::Span;
use serde::Serialize;

/// A parenthesised sub-expression: `( exp )`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpParen {
    pub span: Span,
    pub inner: Box<super::exp::Exp>,
}
