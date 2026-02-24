use serde::Serialize;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatEmpty {
    pub span: Span,
}

impl Parsable for StatEmpty {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_symbol(Symbol::Semi)?;
        Ok(StatEmpty { span: token.span })
    }
}
