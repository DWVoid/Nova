use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VariantMember {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

impl Parsable for VariantMember {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        let type_spec = TypeSpec::parse(p)?;
        let span = name.span.merge(type_spec.span);
        Ok(VariantMember { span, name, type_spec })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VariantDef {
    pub span: Span,
    pub members: Vec<VariantMember>,
}

impl Parsable for VariantDef {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let start = p.expect_keyword(Keyword::Variant)?;
        let mut members = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            members.push(VariantMember::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(VariantDef { span, members })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    #[test]
    fn parses_variant() {
        let v = VariantDef::parse(&mut parser("variant A: int B: str end")).unwrap();
        assert_eq!(v.members.len(), 2);
    }

    #[test]
    fn variant_rejects_missing_end() {
        assert!(VariantDef::parse(&mut parser("variant A: int")).is_err());
    }
}
