use super::stat::Stat;
use crate::lexical::{Span, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatEmpty {
    pub span: Span,
}

impl StatEmpty {
    pub fn new(span: Span) -> Stat {
        Stat::Empty(StatEmpty { span })
    }
}

impl Parsable for StatEmpty {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_symbol(Symbol::Semi)?;
        Ok(StatEmpty { span: token.span })
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
    fn parses_semicolon() {
        assert!(StatEmpty::parse(&mut parser(";")).is_ok());
    }
    #[test]
    fn rejects_non_semi() {
        assert!(StatEmpty::parse(&mut parser("x")).is_err());
    }
}
