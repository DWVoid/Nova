use crate::lexical::Span;
use serde::Serialize;

/// A bare identifier expression: `foo`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpName {
    pub span: Span,
    pub name: String,
}
