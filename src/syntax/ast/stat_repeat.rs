use serde::Serialize;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::block::Block;
use super::exp::Exp;

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
        Ok(StatRepeat { span: token.span.merge(cond.span), block, cond })
    }
}
