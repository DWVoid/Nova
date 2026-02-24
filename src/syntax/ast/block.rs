use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Keyword;
use super::ret_stat::RetStat;
use super::stat::Stat;

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
            end_pos = stat.span.end;
            stats.push(stat);
        }

        let span = Span::new(start, end_pos);
        Ok(Block { span, stats, ret })
    }
}
