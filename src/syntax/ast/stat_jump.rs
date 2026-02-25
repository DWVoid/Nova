use super::stat::Stat;
use crate::lexical::{Keyword, Span};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

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
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
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
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Continue)?;
        Ok(StatContinue { span: token.span })
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
