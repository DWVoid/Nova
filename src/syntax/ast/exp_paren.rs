use super::exp::Exp;
use crate::lexical::Span;
use serde::Serialize;

/// A parenthesised sub-expression: `( exp )`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpParen {
    pub span: Span,
    pub inner: Box<Exp>,
}

impl ExpParen {
    pub fn new(span: Span, inner: Exp) -> Exp {
        Exp::Paren(ExpParen {
            span,
            inner: Box::new(inner),
        })
    }
}
