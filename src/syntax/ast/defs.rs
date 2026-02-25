use super::exp::Exp;
use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldDecl {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

impl Parsable for FieldDecl {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let type_spec = TypeSpec::parse(p)?;
        let span = name.span.merge(type_spec.span);
        Ok(FieldDecl {
            span,
            name,
            type_spec,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StructDef {
    pub span: Span,
    pub fields: Vec<FieldDecl>,
}

impl Parsable for StructDef {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnumMember {
    pub span: Span,
    pub name: Name,
    pub value: Exp,
}

impl Parsable for EnumMember {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
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
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
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
        Ok(EnumDef {
            span,
            type_spec,
            members,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VariantMember {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

impl Parsable for VariantMember {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let type_spec = TypeSpec::parse(p)?;
        let span = name.span.merge(type_spec.span);
        Ok(VariantMember {
            span,
            name,
            type_spec,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VariantDef {
    pub span: Span,
    pub members: Vec<VariantMember>,
}

impl Parsable for VariantDef {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.expect_keyword(Keyword::Variant)?;
        let mut members = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            members.push(VariantMember::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(VariantDef { span, members })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TraitSig {
    pub span: Span,
    pub name: Name,
    pub params: Vec<super::param::Param>,
    pub return_type: TypeSpec,
}

impl Parsable for TraitSig {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let params = p.parse_param_list()?;
        let return_type = TypeSpec::parse(p)?;
        let span = name.span.merge(return_type.span);
        Ok(TraitSig {
            span,
            name,
            params,
            return_type,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TraitDef {
    pub span: Span,
    pub sigs: Vec<TraitSig>,
}

impl Parsable for TraitDef {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.expect_keyword(Keyword::Trait)?;
        let mut sigs = Vec::new();
        while !p.is_keyword(Keyword::End) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            sigs.push(TraitSig::parse(p)?);
            if p.is_symbol(Symbol::Semi) {
                p.advance();
            }
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(TraitDef { span, sigs })
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

    // ── FieldDecl ─────────────────────────────────────────────────────────

    #[test]
    fn parses_field_decl() {
        let f = FieldDecl::parse(&mut parser("x: int")).unwrap();
        assert_eq!(f.name.value, "x");
    }
    #[test]
    fn field_decl_rejects_missing_type() {
        assert!(FieldDecl::parse(&mut parser("x")).is_err());
    }

    // ── StructDef ─────────────────────────────────────────────────────────

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

    // ── EnumDef ───────────────────────────────────────────────────────────

    #[test]
    fn parses_enum() {
        let e = EnumDef::parse(&mut parser("enum: int A = 0 B = 1 end")).unwrap();
        assert_eq!(e.members.len(), 2);
    }
    #[test]
    fn enum_rejects_missing_end() {
        assert!(EnumDef::parse(&mut parser("enum: int A = 0")).is_err());
    }

    // ── VariantDef ────────────────────────────────────────────────────────

    #[test]
    fn parses_variant() {
        let v = VariantDef::parse(&mut parser("variant A: int B: str end")).unwrap();
        assert_eq!(v.members.len(), 2);
    }
    #[test]
    fn variant_rejects_missing_end() {
        assert!(VariantDef::parse(&mut parser("variant A: int")).is_err());
    }

    // ── TraitDef ──────────────────────────────────────────────────────────

    #[test]
    fn parses_empty_trait() {
        let t = TraitDef::parse(&mut parser("trait end")).unwrap();
        assert!(t.sigs.is_empty());
    }
    #[test]
    fn parses_trait_with_sig() {
        let t = TraitDef::parse(&mut parser("trait foo(): unit end")).unwrap();
        assert_eq!(t.sigs.len(), 1);
        assert_eq!(t.sigs[0].name.value, "foo");
    }
    #[test]
    fn trait_rejects_missing_end() {
        assert!(TraitDef::parse(&mut parser("trait foo(): unit")).is_err());
    }
}
