use super::exp::{Exp, ExpKind};
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatCall {
    pub span: Span,
    /// The call expression. Always `ExpKind::Call { .. }`.
    pub call: Exp,
}

impl Parsable for StatCall {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let exp = Exp::parse(p)?;
        match exp.kind {
            ExpKind::Call { .. } => Ok(StatCall {
                span: exp.span,
                call: exp,
            }),
            _ => Err(ParseError {
                message: "expected function call expression".to_string(),
                position: exp.span.start,
            }),
        }
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
    fn parses_call_stat() {
        let s = StatCall::parse(&mut parser("f()")).unwrap();
        assert!(matches!(
            s.call.kind,
            super::super::exp::ExpKind::Call { .. }
        ));
    }
    #[test]
    fn parses_chained_call_stat() {
        assert!(StatCall::parse(&mut parser("a.b()")).is_ok());
    }
    #[test]
    fn rejects_non_call() {
        assert!(StatCall::parse(&mut parser("foo")).is_err());
    }
}
