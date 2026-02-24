use super::ret_stat::RetStat;
use super::stat::Stat;
use crate::lexical::Keyword;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Block {
    pub span: Span,
    pub stats: Vec<Stat>,
    pub ret: Option<RetStat>,
}

impl Parsable for Block {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.current().span.start;
        let mut end_pos = start;
        let mut stats = Vec::new();
        let mut ret = None;

        while !p.is_block_end() {
            if p.is_keyword(Keyword::Return) {
                let retstat = RetStat::parse(p)?;
                end_pos = retstat.span.end;
                ret = Some(retstat);
                if p.is_symbol(crate::lexical::Symbol::Semi) {
                    let semi = p.advance();
                    end_pos = semi.span.end;
                }
                break;
            }
            let stat = Stat::parse(p)?;
            end_pos = stat.span().end;
            stats.push(stat);
        }

        let span = Span::new(start, end_pos);
        Ok(Block { span, stats, ret })
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
    fn parses_empty_block() {
        let mut p = parser("end");
        let b = Block::parse(&mut p).unwrap();
        assert!(b.stats.is_empty());
        assert!(b.ret.is_none());
    }

    #[test]
    fn parses_block_with_return() {
        let mut p = parser("return 1 end");
        let b = Block::parse(&mut p).unwrap();
        assert!(b.ret.is_some());
        assert_eq!(b.ret.unwrap().exprs.len(), 1);
    }

    #[test]
    fn parses_block_with_stats() {
        let mut p = parser("f() g() end");
        let b = Block::parse(&mut p).unwrap();
        assert_eq!(b.stats.len(), 2);
    }

    #[test]
    fn parses_block_stats_then_return() {
        let mut p = parser("f() return 0 end");
        let b = Block::parse(&mut p).unwrap();
        assert_eq!(b.stats.len(), 1);
        assert!(b.ret.is_some());
    }
}
