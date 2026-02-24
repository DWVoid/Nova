use serde::Serialize;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::block::Block;
use super::exp::Exp;
use super::if_clause::IfClause;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatIf {
    pub span: Span,
    pub clauses: Vec<IfClause>,
    pub else_block: Option<Block>,
}

impl Parsable for StatIf {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::If)?;
        let cond = Exp::parse(p)?;
        p.expect_keyword(Keyword::Then)?;
        let block = Block::parse(p)?;
        let mut clauses = vec![IfClause {
            span: token.span.merge(block.span),
            cond,
            block,
        }];
        while p.is_keyword(Keyword::ElseIf) {
            let elseif = p.expect_keyword(Keyword::ElseIf)?;
            let cond = Exp::parse(p)?;
            p.expect_keyword(Keyword::Then)?;
            let block = Block::parse(p)?;
            clauses.push(IfClause {
                span: elseif.span.merge(block.span),
                cond,
                block,
            });
        }
        let else_block = if p.is_keyword(Keyword::Else) {
            p.expect_keyword(Keyword::Else)?;
            Some(Block::parse(p)?)
        } else {
            None
        };
        let end = p.expect_keyword(Keyword::End)?;
        Ok(StatIf { span: token.span.merge(end.span), clauses, else_block })
    }
}
