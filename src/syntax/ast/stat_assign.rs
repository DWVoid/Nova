use serde::Serialize;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::exp::Exp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatAssign {
    pub span: Span,
    /// Left-hand side expressions. The semantic stage validates that each is
    /// a legal l-value (Name, Field, Index, or VarDecl).
    pub vars: Vec<Exp>,
    pub exprs: Vec<Exp>,
}

impl Parsable for StatAssign {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let first = Exp::parse(p)?;
        let mut vars = vec![first];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            vars.push(Exp::parse(p)?);
        }
        let eq = p.expect_symbol(Symbol::Assign)?;
        let exprs = p.parse_exp_list()?;
        let end_span = exprs.last().map(|e| e.span).unwrap_or(eq.span);
        let span = vars.last().map(|v| v.span.merge(end_span)).unwrap_or(end_span);
        Ok(StatAssign { span, vars, exprs })
    }
}