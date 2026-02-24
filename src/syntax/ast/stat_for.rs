use serde::Serialize;
use crate::lexical::{Keyword, Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::block::Block;
use super::exp::Exp;
use super::name::Name;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatForNumeric {
    pub span: Span,
    pub name: Name,
    pub start: Exp,
    pub end: Exp,
    pub step: Option<Exp>,
    pub block: Block,
}

impl Parsable for StatForNumeric {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::For)?;
        let name = Name::parse(p)?;
        p.expect_symbol(Symbol::Assign)?;
        let start = Exp::parse(p)?;
        p.expect_symbol(Symbol::Comma)?;
        let end = Exp::parse(p)?;
        let step = if p.is_symbol(Symbol::Comma) {
            p.advance();
            Some(Exp::parse(p)?)
        } else {
            None
        };
        p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end_kw = p.expect_keyword(Keyword::End)?;
        Ok(StatForNumeric { span: token.span.merge(end_kw.span), name, start, end, step, block })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatForGeneric {
    pub span: Span,
    pub names: Vec<Name>,
    pub exprs: Vec<Exp>,
    pub block: Block,
}

impl Parsable for StatForGeneric {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::For)?;
        let mut names = vec![Name::parse(p)?];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            names.push(Name::parse(p)?);
        }
        p.expect_keyword(Keyword::In)?;
        let exprs = p.parse_exp_list()?;
        p.expect_keyword(Keyword::Do)?;
        let block = Block::parse(p)?;
        let end_kw = p.expect_keyword(Keyword::End)?;
        Ok(StatForGeneric { span: token.span.merge(end_kw.span), names, exprs, block })
    }
}
