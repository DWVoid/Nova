use super::stat::Stat;
use crate::lexical::{Span, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatEmpty {
    pub span: Span,
}

impl StatEmpty {
    pub fn new(span: Span) -> Stat {
        Stat::Empty(StatEmpty { span })
    }
}

impl Parsable for StatEmpty {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_symbol(Symbol::Semi)?;
        Ok(StatEmpty { span: token.span })
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
    fn parses_semicolon() {
        assert!(StatEmpty::parse(&mut parser(";")).is_ok());
    }
    #[test]
    fn rejects_non_semi() {
        assert!(StatEmpty::parse(&mut parser("x")).is_err());
    }
}
