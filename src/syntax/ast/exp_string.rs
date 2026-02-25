use crate::lexical::Span;
use serde::Serialize;

/// A string literal expression: `"hello"`.
/// The value has all escape sequences already resolved to their final form.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpString {
    pub span: Span,
    pub value: String,
}
