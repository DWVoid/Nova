use crate::lexical::Span;
use crate::lexical::TokenKind;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Name {
    pub value: String,
    pub span: Span,
}

impl Parsable for Name {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.current().clone();
        if let TokenKind::Identifier(value) = token.kind {
            p.advance();
            return Ok(Name {
                value,
                span: token.span,
            });
        }
        Err(SyntaxError {
            message: "expected identifier".to_string(),
            position: token.span.start,
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
    fn parses_identifier() {
        let mut p = parser("foo");
        let n = Name::parse(&mut p).unwrap();
        assert_eq!(n.value, "foo");
    }

    #[test]
    fn rejects_non_identifier() {
        let mut p = parser("42");
        assert!(Name::parse(&mut p).is_err());
    }
}
