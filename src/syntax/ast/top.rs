use super::chunk::NamespaceDecl;
use super::decorator::Decorator;
use super::defs::{EnumDef, StructDef, TraitDef, VariantDef};
use super::exp::Exp;
use super::name::Name;
use super::param::Param;
use super::type_spec::{TypeName, TypeSpec};
use super::visibility::Visibility;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum DefExpr {
    Struct(StructDef),
    Enum(EnumDef),
    Variant(VariantDef),
    Trait(TraitDef),
    Exp(Exp),
}

impl DefExpr {
    pub fn span(&self) -> Span {
        match self {
            DefExpr::Struct(def) => def.span,
            DefExpr::Enum(def) => def.span,
            DefExpr::Variant(def) => def.span,
            DefExpr::Trait(def) => def.span,
            DefExpr::Exp(exp) => exp.span(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Definition {
    pub span: Span,
    pub decorators: Vec<Decorator>,
    pub visibility: Option<Visibility>,
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
    pub expr: DefExpr,
}

impl Parsable for Definition {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let decorators = Vec::<Decorator>::parse(p)?;
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Implementation {
    pub span: Span,
    pub trait_type: Option<TypeName>,
    pub target: TypeName,
    pub items: Vec<Definition>,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum TopItem {
    Definition(Definition),
    Implementation(Implementation),
}

impl TopItem {
    pub fn extract_definition(&self) -> Option<Definition> {
        match self {
            TopItem::Definition(def) => Some(def.clone()),
            TopItem::Implementation(_) => None,
        }
    }
}

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

impl Parser {
    /// Parse a dot-separated identifier path, used by `NamespaceDecl` and type names.
    pub(super) fn parse_namespace_path(&mut self) -> Result<Vec<Name>, ParseError> {
        let mut parts = Vec::new();
        parts.push(Name::parse(self)?);
        while self.is_symbol(Symbol::Dot) {
            self.advance();
            parts.push(Name::parse(self)?);
        }
        Ok(parts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::lex;
    use crate::syntax::parser::Parser;

    fn parser(src: &str) -> Parser {
        let r = lex(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    // ── Definition ────────────────────────────────────────────────────────

    #[test]
    fn parses_simple_definition() {
        let d = Definition::parse(&mut parser("define x 42")).unwrap();
        assert_eq!(d.name.value, "x");
        assert!(d.visibility.is_none());
        assert!(d.decorators.is_empty());
        assert!(matches!(d.expr, DefExpr::Exp(_)));
    }

    #[test]
    fn parses_exported_definition() {
        let d = Definition::parse(&mut parser("export define x 0")).unwrap();
        assert!(d.visibility.is_some());
    }

    #[test]
    fn parses_decorated_definition() {
        let d = Definition::parse(&mut parser("@inline define x 0")).unwrap();
        assert_eq!(d.decorators.len(), 1);
    }

    #[test]
    fn parses_struct_definition() {
        let d = Definition::parse(&mut parser("define Point struct x: int y: int end")).unwrap();
        assert!(matches!(d.expr, DefExpr::Struct(_)));
    }

    #[test]
    fn definition_rejects_missing_name() {
        assert!(Definition::parse(&mut parser("define 42")).is_err());
    }

    // ── Implementation ────────────────────────────────────────────────────

    #[test]
    fn parses_impl_for_type() {
        let i = Implementation::parse(&mut parser("implement for Foo end")).unwrap();
        assert!(i.trait_type.is_none());
        assert_eq!(i.target.parts[0].value, "Foo");
    }

    #[test]
    fn parses_trait_impl() {
        let i = Implementation::parse(&mut parser("implement Bar for Foo end")).unwrap();
        assert!(i.trait_type.is_some());
    }

    #[test]
    fn impl_rejects_missing_for() {
        assert!(Implementation::parse(&mut parser("implement Foo end")).is_err());
    }

    #[test]
    fn impl_rejects_missing_end() {
        assert!(Implementation::parse(&mut parser("implement for Foo")).is_err());
    }

    // ── TopItem ───────────────────────────────────────────────────────────

    #[test]
    fn top_item_definition() {
        assert!(matches!(
            TopItem::parse(&mut parser("define x 0")).unwrap(),
            TopItem::Definition(_)
        ));
    }

    #[test]
    fn top_item_implementation() {
        assert!(matches!(
            TopItem::parse(&mut parser("implement for Foo end")).unwrap(),
            TopItem::Implementation(_)
        ));
    }

    #[test]
    fn top_item_rejects_other() {
        assert!(TopItem::parse(&mut parser("namespace Foo;")).is_err());
    }
}