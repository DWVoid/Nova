use crate::lexical::{Keyword, Symbol};
use crate::syntax::ast::{EnumDef, EnumMember, Exp, FieldDecl, Name, StructDef, TraitDef, TraitSig, TypeSpec, VariantDef, VariantMember};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};

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

impl Parsable for EnumMember {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        p.expect_symbol(Symbol::Assign)?;
        let value = Exp::parse(p)?;
        let span = name.span.merge(value.span);
        Ok(EnumMember { span, name, value })
    }
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