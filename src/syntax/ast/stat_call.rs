use super::exp::Exp;
use super::stat::Stat;
use crate::lexical::Span;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatCall {
    pub span: Span,
    /// The call expression. Always `Exp::Call(..)`.
    pub call: Exp,
}

impl StatCall {
    pub fn new(span: Span, call: Exp) -> Stat {
        Stat::Call(StatCall { span, call })
    }
}

impl Parsable for StatCall {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let exp = Exp::parse(p)?;
        if matches!(exp, Exp::Call(_)) {
            let span = exp.span();
            Ok(StatCall { span, call: exp })
        } else {
            Err(SyntaxError {
                message: "expected function call expression".to_string(),
                position: exp.span().start,
            })
        }
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
    fn parses_call_stat() {
        let s = StatCall::parse(&mut parser("f()")).unwrap();
        assert!(matches!(s.call, Exp::Call(_)));
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
