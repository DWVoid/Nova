use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use serde::Serialize;

/// Distinguishes `var` (mutable) from `val` (immutable) at a declaration site.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum VarDeclKind {
    Var,
    Val,
}

/// A local binding site: `var name [: T]` or `val name [: T]`.
///
/// Valid only as an l-value in an assignment statement; the semantic
/// stage enforces this constraint.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpVarDecl {
    pub span: Span,
    pub kind: VarDeclKind,
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
}
