use serde::Serialize;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatBreak {
    pub span: Span,
}

impl Parsable for StatBreak {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Break)?;
        Ok(StatBreak { span: token.span })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatContinue {
    pub span: Span,
}

impl Parsable for StatContinue {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Continue)?;
        Ok(StatContinue { span: token.span })
    }
}
