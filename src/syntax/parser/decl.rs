use super::{ParseError, Parser};
use crate::syntax::ast::{
    Decorator, DefExpr, Definition, EnumDef, EnumMember, FieldDecl, Implementation, Name, Param,
    StructDef, TraitDef, TraitSig, TypeName, TypeSpec, UseDecl, UseItem, UseTail, VariantDef,
    VariantMember, Visibility,
};
use crate::lexical::token::{Keyword, Span, Symbol};

impl Parser {
    pub(super) fn parse_use_decl(&mut self) -> Result<UseDecl, ParseError> {
        let token = self.expect_keyword(Keyword::Use)?;
        let path = self.parse_namespace_path()?;
        let tail = if self.is_symbol(Symbol::Dot) && self.peek_is_symbol(1, Symbol::LBrace) {
            self.advance();
            self.expect_symbol(Symbol::LBrace)?;
            let mut items = Vec::new();
            if !self.is_symbol(Symbol::RBrace) {
                items.push(self.parse_use_item()?);
                while self.is_symbol(Symbol::Comma) {
                    self.advance();
                    items.push(self.parse_use_item()?);
                }
            }
            self.expect_symbol(Symbol::RBrace)?;
            Some(UseTail::Selector(items))
        } else if self.is_keyword(Keyword::As) {
            self.advance();
            let alias = self.parse_name()?;
            Some(UseTail::Alias(alias))
        } else {
            None
        };
        let semi = self.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(UseDecl { span, path, tail })
    }

    pub(super) fn parse_namespace_path(&mut self) -> Result<Vec<Name>, ParseError> {
        let mut parts = Vec::new();
        parts.push(self.parse_name()?);
        while self.is_symbol(Symbol::Dot) {
            self.advance();
            parts.push(self.parse_name()?);
        }
        Ok(parts)
    }

    pub(super) fn parse_definition(&mut self) -> Result<Definition, ParseError> {
        let decorators = self.parse_decorators()?;
        let visibility = if self.is_keyword(Keyword::Export) {
            Some(self.parse_visibility()?)
        } else {
            None
        };
        let def_token = self.expect_keyword(Keyword::Define)?;
        let name = self.parse_name()?;
        let type_spec = if self.is_symbol(Symbol::Colon) {
            Some(self.parse_type_spec()?)
        } else {
            None
        };
        let expr = if self.is_keyword(Keyword::Struct) {
            DefExpr::Struct(self.parse_struct_def()?)
        } else if self.is_keyword(Keyword::Enum) {
            DefExpr::Enum(self.parse_enum_def()?)
        } else if self.is_keyword(Keyword::Variant) {
            DefExpr::Variant(self.parse_variant_def()?)
        } else if self.is_keyword(Keyword::Trait) {
            DefExpr::Trait(self.parse_trait_def()?)
        } else {
            DefExpr::Exp(self.parse_exp(0)?)
        };
        let mut span = def_token.span.merge(expr.span());
        if self.is_symbol(Symbol::Semi) {
            let semi = self.advance();
            span = span.merge(semi.span);
        }
        Ok(Definition {
            span,
            decorators,
            visibility,
            name,
            type_spec,
            expr,
        })
    }

    pub(super) fn parse_implementation(&mut self) -> Result<Implementation, ParseError> {
        let start = self.expect_keyword(Keyword::Implement)?;
        let trait_type = if self.is_keyword(Keyword::For) {
            None
        } else {
            Some(self.parse_type_name()?)
        };
        self.expect_keyword(Keyword::For)?;
        let target = self.parse_type_name()?;
        let mut items = Vec::new();
        while !self.is_keyword(Keyword::End) {
            items.push(self.parse_definition()?);
        }
        let end = self.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(Implementation {
            span,
            trait_type,
            target,
            items,
        })
    }

    fn parse_visibility(&mut self) -> Result<Visibility, ParseError> {
        let token = self.expect_keyword(Keyword::Export)?;
        let scopes = if self.is_symbol(Symbol::LParen) {
            self.advance();
            let mut names = Vec::new();
            if !self.is_symbol(Symbol::RParen) {
                names.push(self.parse_name()?);
                while self.is_symbol(Symbol::Comma) {
                    self.advance();
                    names.push(self.parse_name()?);
                }
            }
            let _close = self.expect_symbol(Symbol::RParen)?;
            Some(names)
        } else {
            None
        };
        let span = token.span;
        Ok(Visibility { span, scopes })
    }

    fn parse_decorators(&mut self) -> Result<Vec<Decorator>, ParseError> {
        let mut decorators = Vec::new();
        while self.is_symbol(Symbol::At) {
            decorators.push(self.parse_decorator()?);
        }
        Ok(decorators)
    }

    fn parse_decorator(&mut self) -> Result<Decorator, ParseError> {
        let at = self.expect_symbol(Symbol::At)?;
        let name = self.parse_name()?;
        let mut span = at.span.merge(name.span);
        let args = if self.is_symbol(Symbol::LParen) {
            self.advance();
            let args = if self.is_symbol(Symbol::RParen) {
                Vec::new()
            } else {
                self.parse_exp_list(0)?
            };
            let close = self.expect_symbol(Symbol::RParen)?;
            span = span.merge(close.span);
            Some(args)
        } else {
            None
        };
        Ok(Decorator { span, name, args })
    }

    fn parse_use_item(&mut self) -> Result<UseItem, ParseError> {
        let name = self.parse_name()?;
        let alias = if self.is_keyword(Keyword::As) {
            self.advance();
            Some(self.parse_name()?)
        } else {
            None
        };
        Ok(UseItem { name, alias })
    }

    pub(super) fn parse_type_spec(&mut self) -> Result<TypeSpec, ParseError> {
        let colon = self.expect_symbol(Symbol::Colon)?;
        let ty = self.parse_type_name()?;
        let span = colon.span.merge(ty.span);
        Ok(TypeSpec { span, ty })
    }

    pub(super) fn parse_type_name(&mut self) -> Result<TypeName, ParseError> {
        let parts = self.parse_namespace_path()?;
        let mut span = parts[0].span;
        for name in &parts[1..] {
            span = span.merge(name.span);
        }
        Ok(TypeName { span, parts })
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, ParseError> {
        let start = self.expect_keyword(Keyword::Struct)?;
        let mut fields = Vec::new();
        while !self.is_keyword(Keyword::End) {
            if self.is_symbol(Symbol::Semi) {
                self.advance();
                continue;
            }
            fields.push(self.parse_field_decl()?);
            if self.is_symbol(Symbol::Semi) {
                self.advance();
            }
        }
        let end = self.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(StructDef { span, fields })
    }

    fn parse_field_decl(&mut self) -> Result<FieldDecl, ParseError> {
        let name = self.parse_name()?;
        let type_spec = self.parse_type_spec()?;
        let span = name.span.merge(type_spec.span);
        Ok(FieldDecl { span, name, type_spec })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDef, ParseError> {
        let start = self.expect_keyword(Keyword::Enum)?;
        let type_spec = self.parse_type_spec()?;
        let mut members = Vec::new();
        while !self.is_keyword(Keyword::End) {
            if self.is_symbol(Symbol::Semi) {
                self.advance();
                continue;
            }
            members.push(self.parse_enum_member()?);
            if self.is_symbol(Symbol::Semi) {
                self.advance();
            }
        }
        let end = self.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(EnumDef {
            span,
            type_spec,
            members,
        })
    }

    fn parse_enum_member(&mut self) -> Result<EnumMember, ParseError> {
        let name = self.parse_name()?;
        let _eq = self.expect_symbol(Symbol::Assign)?;
        let value = self.parse_exp(0)?;
        let span = name.span.merge(value.span);
        Ok(EnumMember { span, name, value })
    }

    fn parse_variant_def(&mut self) -> Result<VariantDef, ParseError> {
        let start = self.expect_keyword(Keyword::Variant)?;
        let mut members = Vec::new();
        while !self.is_keyword(Keyword::End) {
            if self.is_symbol(Symbol::Semi) {
                self.advance();
                continue;
            }
            members.push(self.parse_variant_member()?);
            if self.is_symbol(Symbol::Semi) {
                self.advance();
            }
        }
        let end = self.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(VariantDef { span, members })
    }

    fn parse_variant_member(&mut self) -> Result<VariantMember, ParseError> {
        let name = self.parse_name()?;
        let type_spec = self.parse_type_spec()?;
        let span = name.span.merge(type_spec.span);
        Ok(VariantMember {
            span,
            name,
            type_spec,
        })
    }

    fn parse_trait_def(&mut self) -> Result<TraitDef, ParseError> {
        let start = self.expect_keyword(Keyword::Trait)?;
        let mut sigs = Vec::new();
        while !self.is_keyword(Keyword::End) {
            if self.is_symbol(Symbol::Semi) {
                self.advance();
                continue;
            }
            sigs.push(self.parse_trait_sig()?);
            if self.is_symbol(Symbol::Semi) {
                self.advance();
            }
        }
        let end = self.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(TraitDef { span, sigs })
    }

    fn parse_trait_sig(&mut self) -> Result<TraitSig, ParseError> {
        let name = self.parse_name()?;
        let params = self.parse_param_list()?;
        let return_type = self.parse_type_spec()?;
        let span = name.span.merge(return_type.span);
        Ok(TraitSig {
            span,
            name,
            params,
            return_type,
        })
    }

    pub(super) fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect_symbol(Symbol::LParen)?;
        let mut params = Vec::new();
        if !self.is_symbol(Symbol::RParen) {
            params.push(self.parse_param()?);
            while self.is_symbol(Symbol::Comma) {
                self.advance();
                params.push(self.parse_param()?);
            }
        }
        self.expect_symbol(Symbol::RParen)?;
        Ok(params)
    }

    pub(crate) fn parse_param(&mut self) -> Result<Param, ParseError> {
        let name = self.parse_name()?;
        let type_spec = if self.is_symbol(Symbol::Colon) {
            Some(self.parse_type_spec()?)
        } else {
            None
        };
        Ok(Param { name, type_spec })
    }
}

trait DefExprSpan {
    fn span(&self) -> Span;
}

impl DefExprSpan for DefExpr {
    fn span(&self) -> Span {
        match self {
            DefExpr::Struct(def) => def.span,
            DefExpr::Enum(def) => def.span,
            DefExpr::Variant(def) => def.span,
            DefExpr::Trait(def) => def.span,
            DefExpr::Exp(exp) => exp.span,
        }
    }
}