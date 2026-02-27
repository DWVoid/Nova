use super::exp::Exp;
use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnumMember {
    pub span: Span,
    pub name: Name,
    pub value: Exp,
}

impl Parsable for EnumMember {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        p.expect_symbol(Symbol::Assign)?;
        let value = Exp::parse(p)?;
        let span = name.span.merge(value.span());
        Ok(EnumMember { span, name, value })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnumDef {
    pub span: Span,
    pub type_spec: TypeSpec,
    pub members: Vec<EnumMember>,
}

impl Parsable for EnumDef {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let start = p.expect_keyword(Keyword::Enum)?;
        let type_spec = TypeSpec::parse(p)?;
        let mut members = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            members.push(EnumMember::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(EnumDef { span, type_spec, members })
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
    fn parses_enum() {
        let e = EnumDef::parse(&mut parser("enum: int A = 0 B = 1 end")).unwrap();
        assert_eq!(e.members.len(), 2);
    }

    #[test]
    fn enum_rejects_missing_end() {
        assert!(EnumDef::parse(&mut parser("enum: int A = 0")).is_err());
    }
}
