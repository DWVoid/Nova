use super::block::Block;
use super::exp::Exp;
use super::name::Name;
use super::stat::Stat;
use crate::lexical::{Keyword, Span, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatForNumeric {
    pub span: Span,
    pub name: Name,
    pub start: Exp,
    pub end: Exp,
    pub step: Option<Exp>,
    pub block: Block,
}

impl StatForNumeric {
    pub fn new(
        span: Span,
        name: Name,
        start: Exp,
        end: Exp,
        step: Option<Exp>,
        block: Block,
    ) -> Stat {
        Stat::ForNumeric(StatForNumeric {
            span,
            name,
            start,
            end,
            step,
            block,
        })
    }
}

impl Parsable for StatForNumeric {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::For)?;
        let name = Name::parse(p)?;
        p.expect_symbol(Symbol::Assign)?;
        let start = Exp::parse(p)?;
        p.expect_symbol(Symbol::Comma)?;
        let end = Exp::parse(p)?;
        let step = if p.is_symbol(Symbol::Comma) {
            p.advance();
            Some(Exp::parse(p)?)
        } else {
            None
        };
        p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end_kw = p.expect_keyword(Keyword::End)?;
        Ok(StatForNumeric {
            span: token.span.merge(end_kw.span),
            name,
            start,
            end,
            step,
            block,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatForGeneric {
    pub span: Span,
    pub names: Vec<Name>,
    pub exprs: Vec<Exp>,
    pub block: Block,
}

impl StatForGeneric {
    pub fn new(span: Span, names: Vec<Name>, exprs: Vec<Exp>, block: Block) -> Stat {
        Stat::ForGeneric(StatForGeneric {
            span,
            names,
            exprs,
            block,
        })
    }
}

impl Parsable for StatForGeneric {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::For)?;
        let mut names = vec![Name::parse(p)?];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            names.push(Name::parse(p)?);
        }
        p.expect_keyword(Keyword::In)?;
        let exprs = Vec::<Exp>::parse(p)?;
        p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end_kw = p.expect_keyword(Keyword::End)?;
        Ok(StatForGeneric {
            span: token.span.merge(end_kw.span),
            names,
            exprs,
            block,
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

    // ── StatForNumeric ────────────────────────────────────────────────────

    #[test]
    fn parses_numeric_for_no_step() {
        let s = StatForNumeric::parse(&mut parser("for i = 1, 10 do end")).unwrap();
        assert_eq!(s.name.value, "i");
        assert!(s.step.is_none());
    }

    #[test]
    fn parses_numeric_for_with_step() {
        let s = StatForNumeric::parse(&mut parser("for i = 1, 10, 2 do end")).unwrap();
        assert!(s.step.is_some());
    }

    #[test]
    fn numeric_for_rejects_missing_end() {
        assert!(StatForNumeric::parse(&mut parser("for i = 1, 10 do")).is_err());
    }

    // ── StatForGeneric ────────────────────────────────────────────────────

    #[test]
    fn parses_generic_for_single_name() {
        let s = StatForGeneric::parse(&mut parser("for x in iter do end")).unwrap();
        assert_eq!(s.names.len(), 1);
        assert_eq!(s.exprs.len(), 1);
    }

    #[test]
    fn parses_generic_for_multiple_names() {
        let s = StatForGeneric::parse(&mut parser("for k, v in pairs do end")).unwrap();
        assert_eq!(s.names.len(), 2);
    }

    #[test]
    fn generic_for_rejects_missing_in() {
        assert!(StatForGeneric::parse(&mut parser("for x do end")).is_err());
    }
}
