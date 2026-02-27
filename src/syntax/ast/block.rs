use super::stat::Stat;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Block {
    pub span: Span,
    pub stats: Vec<Stat>,
}

impl Parsable for Block {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.current().span.start;
        let mut end_pos = start;
        let mut stats = Vec::new();

        while !p.is_block_end() {
            let stat = Stat::parse(p)?;
            let is_return = matches!(stat, Stat::Return(_));
            end_pos = stat.span().end;
            stats.push(stat);
            // A return may be followed by an optional ';'; consume it and stop.
            if is_return {
                if p.is_symbol(crate::lexical::Symbol::Semi) {
                    let semi = p.advance();
                    end_pos = semi.span.end;
                }
                break;
            }
        }
        let span = Span::new(start, end_pos);
        Ok(Block { span, stats })
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
    }

    #[test]
    fn parses_block_with_return() {
        let mut p = parser("return 1 end");
        let b = Block::parse(&mut p).unwrap();
        assert_eq!(b.stats.len(), 1);
        assert!(matches!(b.stats[0], Stat::Return(_)));
    }

    #[test]
    fn parses_block_with_return_semi() {
        let mut p = parser("return 1; end");
        let b = Block::parse(&mut p).unwrap();
        assert_eq!(b.stats.len(), 1);
        assert!(matches!(b.stats[0], Stat::Return(_)));
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
        assert_eq!(b.stats.len(), 2);
        assert!(matches!(b.stats[1], Stat::Return(_)));
    }
}
