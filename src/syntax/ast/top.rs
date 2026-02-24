use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Symbol};
use super::defs::{EnumDef, StructDef, TraitDef, VariantDef};
use super::exp::Exp;
use super::name::Name;
use super::param::Param;
use super::type_spec::{TypeName, TypeSpec};

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
        Ok(Visibility { span: token.span, scopes })
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
        Ok(Definition { span, decorators, visibility, name, type_spec, expr })
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
        Ok(Implementation { span, trait_type, target, items })
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
        if p.is_keyword(Keyword::Define)
            || p.is_symbol(Symbol::At)
            || p.is_keyword(Keyword::Export)
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
