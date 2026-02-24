use serde::Serialize;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::exp::Exp;
use super::prefix_exp::{PrefixExp, PrefixExpKind};
use super::var::Var;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatAssign {
    pub span: Span,
    pub vars: Vec<Var>,
    pub exprs: Vec<Exp>,
}

impl Parsable for StatAssign {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        // Parse the first target — must be a Var (not a call or paren)
        let first = PrefixExp::parse(p)?;
        let first_var = match first.kind {
            PrefixExpKind::Var(var) => var,
            PrefixExpKind::Paren(_) => {
                return Err(ParseError {
                    message: "parenthesized expression cannot start an assignment".to_string(),
                    position: first.span.start,
                });
            }
            PrefixExpKind::Call(_) => {
                return Err(ParseError {
                    message: "function call cannot be assignment target; use a variable or field"
                        .to_string(),
                    position: first.span.start,
                });
            }
        };
        let mut vars = vec![first_var];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            let next = PrefixExp::parse(p)?;
            match next.kind {
                PrefixExpKind::Var(var) => vars.push(var),
                _ => {
                    return Err(ParseError {
                        message: "invalid assignment target; expected variable, field, or index"
                            .to_string(),
                        position: next.span.start,
                    });
                }
            }
        }
        let eq = p.expect_symbol(Symbol::Assign)?;
        let exprs = p.parse_exp_list()?;
        let end_span = exprs.last().map(|e| e.span).unwrap_or(eq.span);
        let span = vars.last().map(|v| v.span.merge(end_span)).unwrap_or(end_span);
        Ok(StatAssign { span, vars, exprs })
    }
}
