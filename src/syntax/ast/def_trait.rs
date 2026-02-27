use super::name::Name;
use super::param::Param;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TraitSig {
    pub span: Span,
    pub name: Name,
    pub params: Vec<Param>,
    pub return_type: TypeSpec,
}

impl Parsable for TraitSig {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        let params = p.parse_param_list()?;
        let return_type = TypeSpec::parse(p)?;
        let span = name.span.merge(return_type.span);
        Ok(TraitSig { span, name, params, return_type })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TraitDef {
    pub span: Span,
    pub sigs: Vec<TraitSig>,
}

impl Parsable for TraitDef {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let start = p.expect_keyword(Keyword::Trait)?;
        let mut sigs = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            sigs.push(TraitSig::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(TraitDef { span, sigs })
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
    fn parses_empty_trait() {
        let t = TraitDef::parse(&mut parser("trait end")).unwrap();
        assert!(t.sigs.is_empty());
    }

    #[test]
    fn parses_trait_with_sig() {
        let t = TraitDef::parse(&mut parser("trait foo(): unit end")).unwrap();
        assert_eq!(t.sigs.len(), 1);
        assert_eq!(t.sigs[0].name.value, "foo");
    }

    #[test]
    fn trait_rejects_missing_end() {
        assert!(TraitDef::parse(&mut parser("trait foo(): unit")).is_err());
    }
}
