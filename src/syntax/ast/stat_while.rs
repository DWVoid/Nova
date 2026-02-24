use serde::Serialize;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::block::Block;
use super::exp::Exp;

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
        Ok(StatWhile { span: token.span.merge(end.span), cond, block })
    }
}
