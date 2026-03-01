use super::block::Block;
use super::exp::Exp;
use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IfClause {
    pub span: Span,
    pub cond: Exp,
    pub block: Block,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatIf {
    pub span: Span,
    pub clauses: Vec<IfClause>,
    pub else_block: Option<Block>,
}

impl StatIf {
    pub fn new(span: Span, clauses: Vec<IfClause>, else_block: Option<Block>) -> Stat {
        Stat::If(StatIf {
            span,
            clauses,
            else_block,
        })
    }
}

impl Parsable for StatIf {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
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
        Ok(StatIf {
            span: token.span.merge(end.span),
            clauses,
            else_block,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    #[test]
    fn parses_if_only() {
        let s = StatIf::parse(&mut parser("if true then end")).unwrap();
        assert_eq!(s.clauses.len(), 1);
        assert!(s.else_block.is_none());
    }

    #[test]
    fn parses_if_else() {
        let s = StatIf::parse(&mut parser("if true then else end")).unwrap();
        assert!(s.else_block.is_some());
    }

    #[test]
    fn parses_if_elseif_else() {
        let s = StatIf::parse(&mut parser("if true then elseif false then else end")).unwrap();
        assert_eq!(s.clauses.len(), 2);
        assert!(s.else_block.is_some());
    }

    #[test]
    fn rejects_missing_then() {
        assert!(StatIf::parse(&mut parser("if true end")).is_err());
    }

    #[test]
    fn rejects_missing_end() {
        assert!(StatIf::parse(&mut parser("if true then")).is_err());
    }
}
