use crate::lexical::Span;
use serde::Serialize;

/// The `nil` literal expression.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpNil {
    pub span: Span,
}
