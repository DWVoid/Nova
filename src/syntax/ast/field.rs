use super::exp::Exp;
use super::name::Name;
use crate::lexical::Span;
use crate::lexical::Symbol;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FieldKey {
    Exp(Exp),
    Name(Name),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub span: Span,
    pub key: Option<FieldKey>,
    pub value: Exp,
}

impl Parsable for Field {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        use crate::lexical::TokenKind;
        if p.is_symbol(Symbol::LBracket) {
            let open = p.advance();
            let key = Exp::parse(p)?;
            p.expect_symbol(Symbol::RBracket)?;
            p.expect_symbol(Symbol::Assign)?;
            let value = Exp::parse(p)?;
            let span = open.span.merge(value.span());
            return Ok(Field {
                span,
                key: Some(FieldKey::Exp(key)),
                value,
            });
        }
        if let TokenKind::Identifier(_) = p.current().kind {
            if p.peek_is_symbol(1, Symbol::Assign) {
                let name = Name::parse(p)?;
                p.expect_symbol(Symbol::Assign)?;
                let value = Exp::parse(p)?;
                let span = name.span.merge(value.span());
                return Ok(Field {
                    span,
                    key: Some(FieldKey::Name(name)),
                    value,
                });
            }
        }
        let value = Exp::parse(p)?;
        Ok(Field {
            span: value.span(),
            key: None,
            value,
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
    fn parses_positional_field() {
        let mut p = parser("42");
        let f = Field::parse(&mut p).unwrap();
        assert!(f.key.is_none());
    }

    #[test]
    fn parses_name_key_field() {
        let mut p = parser("x = 1");
        let f = Field::parse(&mut p).unwrap();
        assert!(matches!(f.key, Some(FieldKey::Name(_))));
    }

    #[test]
    fn parses_exp_key_field() {
        let mut p = parser("[0] = 1");
        let f = Field::parse(&mut p).unwrap();
        assert!(matches!(f.key, Some(FieldKey::Exp(_))));
    }

    #[test]
    fn exp_key_rejects_missing_bracket() {
        // `[0 = 1` — missing closing bracket
        let mut p = parser("[0 = 1");
        assert!(Field::parse(&mut p).is_err());
    }
}
