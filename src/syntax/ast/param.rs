use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Symbol;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
}

impl Parsable for Param {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        let type_spec = if p.is_symbol(Symbol::Colon) {
            Some(TypeSpec::parse(p)?)
        } else {
            None
        };
        Ok(Param { name, type_spec })
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
    fn parses_param_no_type() {
        let mut p = parser("x");
        let param = Param::parse(&mut p).unwrap();
        assert_eq!(param.name.value, "x");
        assert!(param.type_spec.is_none());
    }

    #[test]
    fn parses_param_with_type() {
        let mut p = parser("x: Foo");
        let param = Param::parse(&mut p).unwrap();
        assert_eq!(param.name.value, "x");
        assert!(param.type_spec.is_some());
    }

    #[test]
    fn rejects_non_identifier() {
        let mut p = parser("42");
        assert!(Param::parse(&mut p).is_err());
    }
}
