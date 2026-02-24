use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Keyword;
use super::exp::Exp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RetStat {
    pub span: Span,
    pub exprs: Vec<Exp>,
}

impl Parsable for RetStat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !p.is_block_end() && !p.is_symbol(crate::lexical::Symbol::Semi) {
            exprs = p.parse_exp_list()?;
        }
        let end_span = exprs.last().map(|e| e.span).unwrap_or(token.span);
        Ok(RetStat {
            span: token.span.merge(end_span),
            exprs,
        })
    }
}
