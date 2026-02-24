use serde::Serialize;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Symbol;
use super::name::Name;
use super::type_spec::TypeSpec;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Param {
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
}

impl Parsable for Param {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let type_spec = if p.is_symbol(Symbol::Colon) {
            Some(TypeSpec::parse(p)?)
        } else {
            None
        };
        Ok(Param { name, type_spec })
    }
}
