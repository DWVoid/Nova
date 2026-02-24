use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Symbol;
use super::name::Name;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeName {
    pub span: Span,
    pub parts: Vec<Name>,
}

impl Parsable for TypeName {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let parts = p.parse_namespace_path()?;
        let mut span = parts[0].span;
        for name in &parts[1..] {
            span = span.merge(name.span);
        }
        Ok(TypeName { span, parts })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeSpec {
    pub span: Span,
    pub ty: TypeName,
}

impl Parsable for TypeSpec {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let colon = p.expect_symbol(Symbol::Colon)?;
        let ty = TypeName::parse(p)?;
        let span = colon.span.merge(ty.span);
        Ok(TypeSpec { span, ty })
    }
}
