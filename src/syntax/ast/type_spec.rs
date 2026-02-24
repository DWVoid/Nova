use super::name::Name;
use crate::lexical::Span;
use crate::lexical::Symbol;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::lex;
    use crate::syntax::parser::Parser;

    fn parser(src: &str) -> Parser {
        let r = lex(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    // ── TypeName ──────────────────────────────────────────────────────────

    #[test]
    fn parses_simple_type_name() {
        let mut p = parser("Foo");
        let t = TypeName::parse(&mut p).unwrap();
        assert_eq!(t.parts.len(), 1);
        assert_eq!(t.parts[0].value, "Foo");
    }

    #[test]
    fn parses_qualified_type_name() {
        let mut p = parser("Foo.Bar.Baz");
        let t = TypeName::parse(&mut p).unwrap();
        assert_eq!(t.parts.len(), 3);
    }

    #[test]
    fn type_name_rejects_non_identifier() {
        let mut p = parser("42");
        assert!(TypeName::parse(&mut p).is_err());
    }

    // ── TypeSpec ──────────────────────────────────────────────────────────

    #[test]
    fn parses_type_spec() {
        let mut p = parser(": Foo");
        let ts = TypeSpec::parse(&mut p).unwrap();
        assert_eq!(ts.ty.parts[0].value, "Foo");
    }

    #[test]
    fn type_spec_rejects_missing_colon() {
        let mut p = parser("Foo");
        assert!(TypeSpec::parse(&mut p).is_err());
    }
}
