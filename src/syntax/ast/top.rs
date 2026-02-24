use super::defs::{EnumDef, StructDef, TraitDef, VariantDef};
use super::exp::Exp;
use super::name::Name;
use super::param::Param;
use super::type_spec::{TypeName, TypeSpec};
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Visibility {
    pub span: Span,
    pub scopes: Option<Vec<Name>>,
}

impl Parsable for Visibility {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Export)?;
        let scopes = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let mut names = Vec::new();
            if !p.is_symbol(Symbol::RParen) {
                names.push(Name::parse(p)?);
                while p.is_symbol(Symbol::Comma) {
                    p.advance();
                    names.push(Name::parse(p)?);
                }
            }
            p.expect_symbol(Symbol::RParen)?;
            Some(names)
        } else {
            None
        };
        Ok(Visibility {
            span: token.span,
            scopes,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decorator {
    pub span: Span,
    pub name: Name,
    pub args: Option<Vec<Exp>>,
}

impl Parsable for Decorator {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let at = p.expect_symbol(Symbol::At)?;
        let name = Name::parse(p)?;
        let mut span = at.span.merge(name.span);
        let args = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let args = if p.is_symbol(Symbol::RParen) {
                Vec::new()
            } else {
                p.parse_exp_list()?
            };
            let close = p.expect_symbol(Symbol::RParen)?;
            span = span.merge(close.span);
            Some(args)
        } else {
            None
        };
        Ok(Decorator { span, name, args })
    }
}

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
            DefExpr::Exp(exp) => exp.span,
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
pub struct UseItem {
    pub name: Name,
    pub alias: Option<Name>,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum UseTail {
    Selector(Vec<UseItem>),
    Alias(Name),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UseDecl {
    pub span: Span,
    pub path: Vec<Name>,
    pub tail: Option<UseTail>,
}

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
            p.advance();
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamespaceDecl {
    pub span: Span,
    pub path: Vec<Name>,
}

impl Parsable for NamespaceDecl {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Namespace)?;
        let path = p.parse_namespace_path()?;
        let semi = p.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(NamespaceDecl { span, path })
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chunk {
    pub span: Span,
    pub trivia: Vec<crate::lexical::Trivia>,
    pub uses: Vec<UseDecl>,
    pub namespace: NamespaceDecl,
    pub items: Vec<TopItem>,
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

    /// Parse an expression list (helper producing `Vec<Exp>`).
    pub(super) fn parse_exp_list(&mut self) -> Result<Vec<Exp>, ParseError> {
        let mut exprs = vec![Exp::parse(self)?];
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            exprs.push(Exp::parse(self)?);
        }
        Ok(exprs)
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

    // ── Visibility ────────────────────────────────────────────────────────

    #[test]
    fn parses_bare_export() {
        let v = Visibility::parse(&mut parser("export")).unwrap();
        assert!(v.scopes.is_none());
    }
    #[test]
    fn parses_export_with_scopes() {
        let v = Visibility::parse(&mut parser("export(a, b)")).unwrap();
        assert_eq!(v.scopes.unwrap().len(), 2);
    }
    #[test]
    fn visibility_rejects_non_export() {
        assert!(Visibility::parse(&mut parser("define")).is_err());
    }

    // ── Decorator ─────────────────────────────────────────────────────────

    #[test]
    fn parses_decorator_no_args() {
        let d = Decorator::parse(&mut parser("@inline")).unwrap();
        assert_eq!(d.name.value, "inline");
        assert!(d.args.is_none());
    }
    #[test]
    fn parses_decorator_with_args() {
        let d = Decorator::parse(&mut parser("@attr(1, 2)")).unwrap();
        assert_eq!(d.args.unwrap().len(), 2);
    }
    #[test]
    fn parses_decorator_empty_args() {
        let d = Decorator::parse(&mut parser("@attr()")).unwrap();
        assert_eq!(d.args.unwrap().len(), 0);
    }
    #[test]
    fn decorator_rejects_missing_at() {
        assert!(Decorator::parse(&mut parser("inline")).is_err());
    }

    // ── UseDecl ───────────────────────────────────────────────────────────

    #[test]
    fn parses_simple_use() {
        let u = UseDecl::parse(&mut parser("use Foo;")).unwrap();
        assert_eq!(u.path.len(), 1);
        assert!(u.tail.is_none());
    }
    #[test]
    fn parses_use_with_alias() {
        let u = UseDecl::parse(&mut parser("use Foo as F;")).unwrap();
        assert!(matches!(u.tail, Some(UseTail::Alias(_))));
    }
    #[test]
    fn parses_use_with_selector() {
        let u = UseDecl::parse(&mut parser("use Foo.{A, B};")).unwrap();
        assert!(matches!(&u.tail, Some(UseTail::Selector(v)) if v.len() == 2));
    }
    #[test]
    fn use_rejects_missing_semi() {
        assert!(UseDecl::parse(&mut parser("use Foo")).is_err());
    }

    // ── NamespaceDecl ─────────────────────────────────────────────────────

    #[test]
    fn parses_namespace() {
        let n = NamespaceDecl::parse(&mut parser("namespace Foo.Bar;")).unwrap();
        assert_eq!(n.path.len(), 2);
    }
    #[test]
    fn namespace_rejects_missing_semi() {
        assert!(NamespaceDecl::parse(&mut parser("namespace Foo")).is_err());
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
