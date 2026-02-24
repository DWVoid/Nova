use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Symbol, TokenKind};
use super::name::Name;
use super::type_spec::TypeSpec;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum VarDeclKind {
    Var,
    Val,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum VarKind {
    Name(Name),
    Index { prefix: Box<super::prefix_exp::PrefixExp>, index: Box<super::exp::Exp> },
    Field { prefix: Box<super::prefix_exp::PrefixExp>, name: Name },
    Decl { kind: VarDeclKind, name: Name, type_spec: Option<TypeSpec> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Var {
    pub span: Span,
    pub kind: VarKind,
}

impl Parsable for Var {
    /// Parse a single variable base (name or `var`/`val` declaration).
    /// Suffix chaining (`.field`, `[index]`) is handled by `PrefixExp::parse`.
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        match p.current().kind.clone() {
            TokenKind::Identifier(_) => {
                let name = Name::parse(p)?;
                Ok(Var {
                    span: name.span,
                    kind: VarKind::Name(name),
                })
            }
            TokenKind::Keyword(Keyword::Var) | TokenKind::Keyword(Keyword::Val) => {
                let decl_kind = if p.is_keyword(Keyword::Var) {
                    VarDeclKind::Var
                } else {
                    VarDeclKind::Val
                };
                let start = p.advance();
                let name = Name::parse(p)?;
                let type_spec = if p.is_symbol(Symbol::Colon) {
                    Some(TypeSpec::parse(p)?)
                } else {
                    None
                };
                let mut span = start.span.merge(name.span);
                if let Some(spec) = &type_spec {
                    span = span.merge(spec.span);
                }
                Ok(Var {
                    span,
                    kind: VarKind::Decl {
                        kind: decl_kind,
                        name,
                        type_spec,
                    },
                })
            }
            _ => Err(ParseError {
                message: "expected variable name or declaration".to_string(),
                position: p.current().span.start,
            }),
        }
    }
}
