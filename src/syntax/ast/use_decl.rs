use super::name::Name;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UseItem {
    pub name: Name,
    pub alias: Option<Name>,
}

impl Parsable for UseItem {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        let alias = if p.is_keyword(Keyword::As) {
            p.advance();
            Some(Name::parse(p)?)
        } else {
            None
        };
        Ok(UseItem { name, alias })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum UseTail {
    Selector(Vec<UseItem>),
    Alias(Name),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UseDecl {
    pub span: Span,
    pub path: Vec<Name>,
    pub tail: Option<UseTail>,
}

impl Parsable for UseDecl {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Use)?;
        let mut path = Vec::new();
        path.push(Name::parse(p)?);
        while p.is_symbol(Symbol::Dot) && !p.peek_is_symbol(1, Symbol::LBrace) {
            p.advance();
            path.push(Name::parse(p)?);
        }
        let tail = if p.is_symbol(Symbol::Dot) && p.peek_is_symbol(1, Symbol::LBrace) {
            p.advance();
            p.expect_symbol(Symbol::LBrace)?;
            let mut items = Vec::new();
            if !p.is_symbol(Symbol::RBrace) {
                items.push(UseItem::parse(p)?);
                while p.is_symbol(Symbol::Comma) {
                    p.advance();
                    items.push(UseItem::parse(p)?);
                }
            }
            p.expect_symbol(Symbol::RBrace)?;
            Some(UseTail::Selector(items))
        } else if p.is_keyword(Keyword::As) {
            p.advance();
            Some(UseTail::Alias(Name::parse(p)?))
        } else {
            None
        };
        let semi = p.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(UseDecl { span, path, tail })
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
    fn parses_simple_use() {
        let u = UseDecl::parse(&mut parser("use Foo;")).unwrap();
        assert_eq!(u.path.len(), 1);
        assert!(u.tail.is_none());
    }

    #[test]
    fn parses_use_with_alias() {
        let u = UseDecl::parse(&mut parser("use Foo as F;")).unwrap();
        assert!(matches!(u.tail, Some(UseTail::Alias(_))));
    }

    #[test]
    fn parses_use_with_selector() {
        let u = UseDecl::parse(&mut parser("use Foo.{A, B};")).unwrap();
        assert!(matches!(&u.tail, Some(UseTail::Selector(v)) if v.len() == 2));
    }

    #[test]
    fn use_rejects_missing_semi() {
        assert!(UseDecl::parse(&mut parser("use Foo")).is_err());
    }
}
