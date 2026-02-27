use super::block::Block;
use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatDo {
    pub span: Span,
    pub block: Block,
}

impl StatDo {
    pub fn new(span: Span, block: Block) -> Stat {
        Stat::Do(StatDo { span, block })
    }
}

impl Parsable for StatDo {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end = p.expect_keyword(Keyword::End)?;
        Ok(StatDo {
            span: token.span.merge(end.span),
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
    fn parses_do_block() {
        let s = StatDo::parse(&mut parser("do end")).unwrap();
        assert!(s.block.stats.is_empty());
    }
    #[test]
    fn rejects_missing_end() {
        assert!(StatDo::parse(&mut parser("do")).is_err());
    }
    #[test]
    fn rejects_missing_do() {
        assert!(StatDo::parse(&mut parser("end")).is_err());
    }
}
