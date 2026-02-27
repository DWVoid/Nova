use super::exp::Exp;
use super::name::Name;
use crate::lexical::Span;
use crate::lexical::Symbol;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decorator {
    pub span: Span,
    pub name: Name,
    pub args: Option<Vec<Exp>>,
}

impl Parsable for Decorator {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let at = p.expect_symbol(Symbol::At)?;
        let name = Name::parse(p)?;
        let mut span = at.span.merge(name.span);
        let args = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let args = if p.is_symbol(Symbol::RParen) {
                Vec::new()
            } else {
                Vec::<Exp>::parse(p)?
            };
            let close = p.expect_symbol(Symbol::RParen)?;
            span = span.merge(close.span);
            Some(args)
        } else {
            None
        };
        Ok(Decorator { span, name, args })
    }
}

/// Parse a (possibly empty) run of `@name` / `@name(…)` decorator annotations.
/// Stops as soon as the current token is not `@`.
impl Parsable for Vec<Decorator> {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let mut decorators = Vec::new();
        while p.is_symbol(Symbol::At) {
            decorators.push(Decorator::parse(p)?);
        }
        Ok(decorators)
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

    // ── Decorator ─────────────────────────────────────────────────────────

    #[test]
    fn parses_decorator_no_args() {
        let d = Decorator::parse(&mut parser("@inline")).unwrap();
        assert_eq!(d.name.value, "inline");
        assert!(d.args.is_none());
    }

    #[test]
    fn parses_decorator_with_args() {
        let d = Decorator::parse(&mut parser("@attr(1, 2)")).unwrap();
        assert_eq!(d.args.unwrap().len(), 2);
    }

    #[test]
    fn parses_decorator_empty_args() {
        let d = Decorator::parse(&mut parser("@attr()")).unwrap();
        assert_eq!(d.args.unwrap().len(), 0);
    }

    #[test]
    fn decorator_rejects_missing_at() {
        assert!(Decorator::parse(&mut parser("inline")).is_err());
    }

    // ── Vec<Decorator> ────────────────────────────────────────────────────

    #[test]
    fn parses_empty_decorator_list() {
        let ds = Vec::<Decorator>::parse(&mut parser("define x 0")).unwrap();
        assert!(ds.is_empty());
    }

    #[test]
    fn parses_single_decorator() {
        let ds = Vec::<Decorator>::parse(&mut parser("@inline define x 0")).unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].name.value, "inline");
    }

    #[test]
    fn parses_multiple_decorators() {
        let ds = Vec::<Decorator>::parse(&mut parser("@a @b(1) @c define x 0")).unwrap();
        assert_eq!(ds.len(), 3);
        assert_eq!(ds[1].args.as_ref().unwrap().len(), 1);
    }
}
