use super::exp::Exp;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

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
        let span = vars
            .last()
            .map(|v| v.span.merge(end_span))
            .unwrap_or(end_span);
        Ok(StatAssign { span, vars, exprs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::lex;
    use crate::syntax::parser::Parser;

    fn parser(src: &str) -> Parser {
        let r = lex(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    #[test]
    fn parses_simple_assign() {
        let s = StatAssign::parse(&mut parser("x = 1")).unwrap();
        assert_eq!(s.vars.len(), 1);
        assert_eq!(s.exprs.len(), 1);
    }

    #[test]
    fn parses_multi_assign() {
        let s = StatAssign::parse(&mut parser("x, y = 1, 2")).unwrap();
        assert_eq!(s.vars.len(), 2);
        assert_eq!(s.exprs.len(), 2);
    }

    #[test]
    fn parses_var_decl_assign() {
        let s = StatAssign::parse(&mut parser("var x = 1")).unwrap();
        assert!(matches!(
            s.vars[0].kind,
            super::super::exp::ExpKind::VarDecl { .. }
        ));
    }

    #[test]
    fn rejects_missing_equals() {
        assert!(StatAssign::parse(&mut parser("x")).is_err());
    }
}
