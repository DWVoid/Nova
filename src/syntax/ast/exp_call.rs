use super::args::Args;
use crate::lexical::Span;
use serde::Serialize;

/// A function call expression: `prefix(args)`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpCall {
    pub span: Span,
    pub prefix: Box<super::exp::Exp>,
    pub args: Args,
}
