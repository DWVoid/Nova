use crate::lexical::Span;
use serde::Serialize;

/// A boolean literal expression: `true` or `false`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpBool {
    pub span: Span,
    pub value: bool,
}
