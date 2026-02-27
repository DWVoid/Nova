use super::block::Block;
use super::exp::Exp;
use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatRepeat {
    pub span: Span,
    pub block: Block,
    pub cond: Exp,
}

impl StatRepeat {
    pub fn new(span: Span, block: Block, cond: Exp) -> Stat {
        Stat::Repeat(StatRepeat { span, block, cond })
    }
}

impl Parsable for StatRepeat {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Repeat)?;
        let block = Block::parse(p)?;
        p.expect_keyword(Keyword::Until)?;
        let cond = Exp::parse(p)?;
        Ok(StatRepeat {
            span: token.span.merge(cond.span()),
            block,
            cond,
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
    fn parses_repeat() {
        let s = StatRepeat::parse(&mut parser("repeat until true")).unwrap();
        assert!(matches!(
            s.cond,
            super::super::exp::Exp::Bool(super::super::exp_bool::ExpBool { value: true, .. })
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
