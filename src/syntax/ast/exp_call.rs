use super::args::Args;
use super::exp::Exp;
use crate::lexical::Span;
use serde::Serialize;

/// A function call expression: `prefix(args)`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpCall {
    pub span: Span,
    pub prefix: Box<super::exp::Exp>,
    pub args: Args,
}

impl ExpCall {
    pub fn new(span: Span, prefix: Exp, args: Args) -> Exp {
        Exp::Call(ExpCall {
            span,
            prefix: Box::new(prefix),
            args,
        })
    }
}
