use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatBreak {
    pub span: Span,
}

impl StatBreak {
    pub fn new(span: Span) -> Stat {
        Stat::Break(StatBreak { span })
    }
}

impl Parsable for StatBreak {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Break)?;
        Ok(StatBreak { span: token.span })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatContinue {
    pub span: Span,
}

impl StatContinue {
    pub fn new(span: Span) -> Stat {
        Stat::Continue(StatContinue { span })
    }
}

impl Parsable for StatContinue {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Continue)?;
        Ok(StatContinue { span: token.span })
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
    fn parses_break() {
        assert!(StatBreak::parse(&mut parser("break")).is_ok());
    }
    #[test]
    fn break_rejects_other() {
        assert!(StatBreak::parse(&mut parser("x")).is_err());
    }

    #[test]
    fn parses_continue() {
        assert!(StatContinue::parse(&mut parser("continue")).is_ok());
    }
    #[test]
    fn continue_rejects_other() {
        assert!(StatContinue::parse(&mut parser("x")).is_err());
    }
}
