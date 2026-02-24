use serde::Serialize;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::block::Block;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatDo {
    pub span: Span,
    pub block: Block,
}

impl Parsable for StatDo {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end = p.expect_keyword(Keyword::End)?;
        Ok(StatDo { span: token.span.merge(end.span), block })
    }
}
