use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDecl {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

impl Parsable for FieldDecl {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let name = Name::parse(p)?;
        let type_spec = TypeSpec::parse(p)?;
        let span = name.span.merge(type_spec.span);
        Ok(FieldDecl { span, name, type_spec })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub span: Span,
    pub fields: Vec<FieldDecl>,
}

impl Parsable for StructDef {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let start = p.expect_keyword(Keyword::Struct)?;
        let mut fields = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            fields.push(FieldDecl::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(StructDef { span, fields })
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
    fn parses_field_decl() {
        let f = FieldDecl::parse(&mut parser("x: int")).unwrap();
        assert_eq!(f.name.value, "x");
    }

    #[test]
    fn field_decl_rejects_missing_type() {
        assert!(FieldDecl::parse(&mut parser("x")).is_err());
    }

    #[test]
    fn parses_empty_struct() {
        let s = StructDef::parse(&mut parser("struct end")).unwrap();
        assert!(s.fields.is_empty());
    }

    #[test]
    fn parses_struct_with_fields() {
        let s = StructDef::parse(&mut parser("struct x: int y: str end")).unwrap();
        assert_eq!(s.fields.len(), 2);
    }

    #[test]
    fn struct_rejects_missing_end() {
        assert!(StructDef::parse(&mut parser("struct x: int")).is_err());
    }
}
