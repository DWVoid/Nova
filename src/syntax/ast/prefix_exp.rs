use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Symbol, TokenKind};
use super::args::Args;
use super::function_call::FunctionCall;
use super::name::Name;
use super::type_spec::TypeSpec;
use super::var::{Var, VarDeclKind, VarKind};
use super::exp::Exp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum PrefixExpKind {
    Var(Var),
    Call(FunctionCall),
    Paren(Box<Exp>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrefixExp {
    pub span: Span,
    pub kind: PrefixExpKind,
}

impl Parsable for PrefixExp {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let mut base = match p.current().kind.clone() {
            TokenKind::Identifier(_) => {
                let name = Name::parse(p)?;
                let var = Var {
                    span: name.span,
                    kind: VarKind::Name(name),
                };
                PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                }
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
                let var = Var {
                    span,
                    kind: VarKind::Decl {
                        kind: decl_kind,
                        name,
                        type_spec,
                    },
                };
                PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                }
            }
            TokenKind::Symbol(Symbol::LParen) => {
                let open = p.advance();
                let expr = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RParen)?;
                PrefixExp {
                    span: open.span.merge(close.span),
                    kind: PrefixExpKind::Paren(Box::new(expr)),
                }
            }
            _ => {
                return Err(ParseError {
                    message: "expected name, declaration, or '('".to_string(),
                    position: p.current().span.start,
                });
            }
        };
        loop {
            if p.is_symbol(Symbol::Dot) {
                p.advance();
                let name = Name::parse(p)?;
                let var = Var {
                    span: base.span.merge(name.span),
                    kind: VarKind::Field {
                        prefix: Box::new(base),
                        name,
                    },
                };
                base = PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                };
                continue;
            }
            if p.is_symbol(Symbol::LBracket) {
                p.advance();
                let index = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RBracket)?;
                let var = Var {
                    span: base.span.merge(close.span),
                    kind: VarKind::Index {
                        prefix: Box::new(base),
                        index: Box::new(index),
                    },
                };
                base = PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                };
                continue;
            }
            if p.is_symbol(Symbol::Colon) {
                p.advance();
                let method = Name::parse(p)?;
                let args = Args::parse(p)?;
                let span = base.span.merge(args.span);
                let call = FunctionCall {
                    span,
                    prefix: Box::new(base),
                    method: Some(method),
                    args,
                };
                base = PrefixExp {
                    span: call.span,
                    kind: PrefixExpKind::Call(call),
                };
                continue;
            }
            if matches!(
                p.current().kind,
                TokenKind::Symbol(Symbol::LParen) | TokenKind::Symbol(Symbol::LBrace)
            ) {
                let args = Args::parse(p)?;
                let span = base.span.merge(args.span);
                let call = FunctionCall {
                    span,
                    prefix: Box::new(base),
                    method: None,
                    args,
                };
                base = PrefixExp {
                    span: call.span,
                    kind: PrefixExpKind::Call(call),
                };
                continue;
            }
            break;
        }
        Ok(base)
    }
}
