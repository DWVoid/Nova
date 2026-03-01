use super::field::Field;
use crate::lexical::Span;
use crate::lexical::Symbol;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Initializer {
    pub span: Span,
    pub fields: Vec<Field>,
}

impl Parsable for Initializer {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let open = p.expect_symbol(Symbol::LBrace)?;
        let mut fields = Vec::new();
        while !p.is_symbol(Symbol::RBrace) {
            fields.push(Field::parse(p)?);
            if p.is_symbol(Symbol::Comma) {
                p.advance();
            } else {
                break;
            }
        }
        let close = p.expect_symbol(Symbol::RBrace)?;
        let span = open.span.merge(close.span);
        Ok(Initializer { span, fields })
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
    fn parses_empty_initializer() {
        let mut p = parser("{}");
        let init = Initializer::parse(&mut p).unwrap();
        assert!(init.fields.is_empty());
    }

    #[test]
    fn parses_fields_with_trailing_comma() {
        let mut p = parser("{1, 2,}");
        let init = Initializer::parse(&mut p).unwrap();
        assert_eq!(init.fields.len(), 2);
    }

    #[test]
    fn parses_fields_without_trailing_comma() {
        let mut p = parser("{1, 2}");
        let init = Initializer::parse(&mut p).unwrap();
        assert_eq!(init.fields.len(), 2);
    }

    #[test]
    fn rejects_missing_brace() {
        let mut p = parser("1, 2}");
        assert!(Initializer::parse(&mut p).is_err());
    }
}
