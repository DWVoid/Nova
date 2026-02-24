use serde::Serialize;
use crate::lexical::Span;
use super::block::Block;
use super::exp::Exp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IfClause {
    pub span: Span,
    pub cond: Exp,
    pub block: Block,
}
