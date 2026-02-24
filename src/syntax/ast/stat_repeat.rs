use super::block::Block;
use super::exp::Exp;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatRepeat {
    pub span: Span,
    pub block: Block,
    pub cond: Exp,
}

impl Parsable for StatRepeat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Repeat)?;
        let block = Block::parse(p)?;
        p.expect_keyword(Keyword::Until)?;
        let cond = Exp::parse(p)?;
        Ok(StatRepeat {
            span: token.span.merge(cond.span),
            block,
            cond,
        })
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

    #[test]
    fn parses_repeat() {
        let s = StatRepeat::parse(&mut parser("repeat until true")).unwrap();
        assert!(matches!(
            s.cond.kind,
            super::super::exp::ExpKind::Bool(true)
        ));
    }
    #[test]
    fn rejects_missing_until() {
        assert!(StatRepeat::parse(&mut parser("repeat true")).is_err());
    }
    #[test]
    fn rejects_missing_repeat() {
        assert!(StatRepeat::parse(&mut parser("until true")).is_err());
    }
}
