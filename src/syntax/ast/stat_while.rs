use super::block::Block;
use super::exp::Exp;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatWhile {
    pub span: Span,
    pub cond: Exp,
    pub block: Block,
}

impl Parsable for StatWhile {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::While)?;
        let cond = Exp::parse(p)?;
        p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end = p.expect_keyword(Keyword::End)?;
        Ok(StatWhile {
            span: token.span.merge(end.span),
            cond,
            block,
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
    fn parses_while() {
        let s = StatWhile::parse(&mut parser("while true do end")).unwrap();
        assert!(matches!(s.cond, super::super::exp::Exp::Bool(super::super::exp_bool::ExpBool { value: true, .. })));
    }
    #[test]
    fn rejects_missing_do() {
        assert!(StatWhile::parse(&mut parser("while true end")).is_err());
    }
    #[test]
    fn rejects_missing_end() {
        assert!(StatWhile::parse(&mut parser("while true do")).is_err());
    }
}
