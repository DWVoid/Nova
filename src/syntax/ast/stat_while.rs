use super::block::Block;
use super::exp::Exp;
use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatWhile {
    pub span: Span,
    pub cond: Exp,
    pub block: Block,
}

impl StatWhile {
    pub fn new(span: Span, cond: Exp, block: Block) -> Stat {
        Stat::While(StatWhile { span, cond, block })
    }
}

impl Parsable for StatWhile {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
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
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    #[test]
    fn parses_while() {
        let s = StatWhile::parse(&mut parser("while true do end")).unwrap();
        assert!(matches!(
            s.cond,
            super::super::exp::Exp::Bool(super::super::exp_bool::ExpBool { value: true, .. })
        ));
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
