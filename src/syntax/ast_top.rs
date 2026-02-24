use super::parsable::Parsable;
/// Collection helpers that remain on `Parser` because they produce `Vec<T>`,
/// not a single named AST node, or are tiny shared building-blocks.
///
/// Individual node parsing has been moved to `impl Parsable for T` in `parsable.rs`.
use super::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Symbol};
use crate::syntax::ast::{
    Decorator, Definition, EnumDef, Exp, Implementation, Name, NamespaceDecl, Param, StructDef,
    TopItem, TraitDef, TypeName, TypeSpec, UseDecl, UseItem, UseTail, VariantDef, Visibility,
};

impl Parser {
    /// Parse a dot-separated identifier path, used by both `NamespaceDecl` and type names.
    pub(super) fn parse_namespace_path(&mut self) -> Result<Vec<Name>, ParseError> {
        let mut parts = Vec::new();
        parts.push(Name::parse(self)?);
        while self.is_symbol(Symbol::Dot) {
            self.advance();
            parts.push(Name::parse(self)?);
        }
        Ok(parts)
    }

    /// Parse zero or more `@decorator` annotations before a definition.
    pub(super) fn parse_decorators(&mut self) -> Result<Vec<Decorator>, ParseError> {
        let mut decorators = Vec::new();
        while self.is_symbol(Symbol::At) {
            decorators.push(Decorator::parse(self)?);
        }
        Ok(decorators)
    }

    /// Parse a `(param, …)` parameter list.
    pub(super) fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect_symbol(Symbol::LParen)?;
        let mut params = Vec::new();
        if !self.is_symbol(Symbol::RParen) {
            params.push(Param::parse(self)?);
            while self.is_symbol(Symbol::Comma) {
                self.advance();
                params.push(Param::parse(self)?);
            }
        }
        self.expect_symbol(Symbol::RParen)?;
        Ok(params)
    }
}

// ---------------------------------------------------------------------------
// Top-level items
// ---------------------------------------------------------------------------

impl Parsable for TopItem {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        if p.is_keyword(Keyword::Define) || p.is_symbol(Symbol::At) || p.is_keyword(Keyword::Export)
        {
            return Ok(TopItem::Definition(Definition::parse(p)?));
        }
        if p.is_keyword(Keyword::Implement) {
            return Ok(TopItem::Implementation(Implementation::parse(p)?));
        }
        Err(ParseError {
            message: "expected top-level definition or implementation".to_string(),
            position: p.current().span.start,
        })
    }
}

// ---------------------------------------------------------------------------
// Use declarations
// ---------------------------------------------------------------------------

impl Parsable for UseDecl {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Use)?;

        let mut path = Vec::new();
        path.push(Name::parse(p)?);

        while p.is_symbol(Symbol::Dot) && !p.peek_is_symbol(1, Symbol::LBrace) {
            p.advance();
            path.push(Name::parse(p)?);
        }

        let tail = if p.is_symbol(Symbol::Dot) && p.peek_is_symbol(1, Symbol::LBrace) {
            p.advance(); // consume the dot
            p.expect_symbol(Symbol::LBrace)?;
            let mut items = Vec::new();
            if !p.is_symbol(Symbol::RBrace) {
                items.push(UseItem::parse(p)?);
                while p.is_symbol(Symbol::Comma) {
                    p.advance();
                    items.push(UseItem::parse(p)?);
                }
            }
            p.expect_symbol(Symbol::RBrace)?;
            Some(UseTail::Selector(items))
        } else if p.is_keyword(Keyword::As) {
            p.advance();
            Some(UseTail::Alias(Name::parse(p)?))
        } else {
            None
        };

        let semi = p.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(UseDecl { span, path, tail })
    }
}

impl Parsable for UseItem {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let alias = if p.is_keyword(Keyword::As) {
            p.advance();
            Some(Name::parse(p)?)
        } else {
            None
        };
        Ok(UseItem { name, alias })
    }
}

// ---------------------------------------------------------------------------
// Namespace declaration
// ---------------------------------------------------------------------------

impl Parsable for NamespaceDecl {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Namespace)?;
        let path = p.parse_namespace_path()?;
        let semi = p.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(NamespaceDecl { span, path })
    }
}

// ---------------------------------------------------------------------------
// Top-level definition and implementation
// ---------------------------------------------------------------------------

impl Parsable for Definition {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let decorators = p.parse_decorators()?;
        let visibility = if p.is_keyword(Keyword::Export) {
            Some(Visibility::parse(p)?)
        } else {
            None
        };
        let def_token = p.expect_keyword(Keyword::Define)?;
        let name = Name::parse(p)?;
        let type_spec = if p.is_symbol(Symbol::Colon) {
            Some(TypeSpec::parse(p)?)
        } else {
            None
        };
        use crate::syntax::ast::DefExpr;
        let expr = if p.is_keyword(Keyword::Struct) {
            DefExpr::Struct(StructDef::parse(p)?)
        } else if p.is_keyword(Keyword::Enum) {
            DefExpr::Enum(EnumDef::parse(p)?)
        } else if p.is_keyword(Keyword::Variant) {
            DefExpr::Variant(VariantDef::parse(p)?)
        } else if p.is_keyword(Keyword::Trait) {
            DefExpr::Trait(TraitDef::parse(p)?)
        } else {
            DefExpr::Exp(Exp::parse(p)?)
        };
        let mut span = def_token.span.merge(expr.span());
        if p.is_symbol(Symbol::Semi) {
            let semi = p.advance();
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
}

impl Parsable for Implementation {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.expect_keyword(Keyword::Implement)?;
        let trait_type = if p.is_keyword(Keyword::For) {
            None
        } else {
            Some(TypeName::parse(p)?)
        };
        p.expect_keyword(Keyword::For)?;
        let target = TypeName::parse(p)?;
        let mut items = Vec::new();
        while !p.is_keyword(Keyword::End) {
            items.push(Definition::parse(p)?);
        }
        let end = p.expect_keyword(Keyword::End)?;
        let span = start.span.merge(end.span);
        Ok(Implementation {
            span,
            trait_type,
            target,
            items,
        })
    }
}
