use super::name::Name;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Visibility {
    pub span: Span,
    pub scopes: Option<Vec<Name>>,
}

impl Parsable for Visibility {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Export)?;
        let scopes = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let mut names = Vec::new();
            if !p.is_symbol(Symbol::RParen) {
                names.push(Name::parse(p)?);
                while p.is_symbol(Symbol::Comma) {
                    p.advance();
                    names.push(Name::parse(p)?);
                }
            }
            p.expect_symbol(Symbol::RParen)?;
            Some(names)
        } else {
            None
        };
        Ok(Visibility {
            span: token.span,
            scopes,
        })
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
    fn parses_bare_export() {
        let v = Visibility::parse(&mut parser("export")).unwrap();
        assert!(v.scopes.is_none());
    }

    #[test]
    fn parses_export_with_scopes() {
        let v = Visibility::parse(&mut parser("export(a, b)")).unwrap();
        assert_eq!(v.scopes.unwrap().len(), 2);
    }

    #[test]
    fn visibility_rejects_non_export() {
        assert!(Visibility::parse(&mut parser("define")).is_err());
    }
}
