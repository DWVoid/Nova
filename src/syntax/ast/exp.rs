use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{Assoc, ParseError, Parser};
use crate::lexical::{Keyword, Symbol, TokenKind};
use super::args::Args;
use super::lambda_expr::LambdaExpr;
use super::name::Name;
use super::ops::{BinOp, UnOp};
use super::type_spec::TypeSpec;

/// Distinguishes `var` (mutable) from `val` (immutable) at a declaration site.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum VarDeclKind {
    Var,
    Val,
}

/// A fully unified expression node.
///
/// The forms previously split between `Exp`/`PrefixExp`/`Var`/`FunctionCall`
/// all live here.  Whether a given node is a valid l-value, a callable
/// expression, or a pure value is not checked by the parser — that
/// distinction is deferred entirely to the semantic stage.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ExpKind {
    // ── Literals ─────────────────────────────────────────────────────────
    Nil,
    Bool(bool),
    Number(String),
    String(String),

    // ── Atomic / postfix forms ────────────────────────────────────────────
    /// A bare identifier: `foo`.
    Name(String),
    /// A parenthesised sub-expression: `( exp )`.
    Paren(Box<Exp>),
    /// Field access: `prefix.name`.
    Field { prefix: Box<Exp>, name: Name },
    /// Index access: `prefix[index]`.
    Index { prefix: Box<Exp>, index: Box<Exp> },
    /// A function call: `prefix(args)`.
    Call { prefix: Box<Exp>, args: Args },
    /// A local binding site: `var name [: T]` or `val name [: T]`.
    /// Valid only as an l-value in an assignment statement; the semantic
    /// stage enforces this constraint.
    VarDecl { kind: VarDeclKind, name: Name, type_spec: Option<TypeSpec> },

    // ── Operator forms ────────────────────────────────────────────────────
    Lambda(LambdaExpr),
    Unary { op: UnOp, exp: Box<Exp> },
    Binary { op: BinOp, left: Box<Exp>, right: Box<Exp> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Exp {
    pub span: Span,
    pub kind: ExpKind,
}

// ── Parsable ──────────────────────────────────────────────────────────────────

impl Parsable for Exp {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        Self::parse_prec(p, 0)
    }
}

impl Exp {
    // ── Pratt driver ─────────────────────────────────────────────────────

    fn parse_prec(p: &mut Parser, min_prec: u8) -> Result<Exp, ParseError> {
        let mut left = Self::parse_unary(p)?;
        loop {
            let Some((op, prec, assoc)) = p.peek_binop() else { break };
            if prec < min_prec { break }
            p.advance();
            let next_min = if assoc == Assoc::Left { prec + 1 } else { prec };
            let right = Self::parse_prec(p, next_min)?;
            let span = left.span.merge(right.span);
            left = Exp {
                span,
                kind: ExpKind::Binary { op, left: Box::new(left), right: Box::new(right) },
            };
        }
        Ok(left)
    }

    fn parse_unary(p: &mut Parser) -> Result<Exp, ParseError> {
        if let Some(op) = p.peek_unop() {
            let token = p.advance();
            let exp = Self::parse_unary(p)?;
            let span = token.span.merge(exp.span);
            return Ok(Exp { span, kind: ExpKind::Unary { op, exp: Box::new(exp) } });
        }
        Self::parse_postfix(p)
    }

    // ── Primary + postfix suffix chaining ────────────────────────────────

    fn parse_postfix(p: &mut Parser) -> Result<Exp, ParseError> {
        let mut base = Self::parse_primary(p)?;
        loop {
            if p.is_symbol(Symbol::Dot) {
                p.advance();
                let name = Name::parse(p)?;
                let span = base.span.merge(name.span);
                base = Exp { span, kind: ExpKind::Field { prefix: Box::new(base), name } };
                continue;
            }
            if p.is_symbol(Symbol::LBracket) {
                p.advance();
                let index = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RBracket)?;
                let span = base.span.merge(close.span);
                base = Exp { span, kind: ExpKind::Index { prefix: Box::new(base), index: Box::new(index) } };
                continue;
            }
            if matches!(p.current().kind,
                TokenKind::Symbol(Symbol::LParen) | TokenKind::Symbol(Symbol::LBrace))
            {
                let args = Args::parse(p)?;
                let span = base.span.merge(args.span);
                base = Exp { span, kind: ExpKind::Call { prefix: Box::new(base), args } };
                continue;
            }
            break;
        }
        Ok(base)
    }

    fn parse_primary(p: &mut Parser) -> Result<Exp, ParseError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Number(text) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::Number(text) })
            }
            TokenKind::StringLiteral(text) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::String(text) })
            }
            TokenKind::Keyword(Keyword::Nil) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::Nil })
            }
            TokenKind::Keyword(Keyword::True) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::Bool(true) })
            }
            TokenKind::Keyword(Keyword::False) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::Bool(false) })
            }
            TokenKind::Identifier(name) => {
                p.advance();
                Ok(Exp { span: token.span, kind: ExpKind::Name(name) })
            }
            TokenKind::Keyword(Keyword::Var) | TokenKind::Keyword(Keyword::Val) => {
                let decl_kind = if matches!(token.kind, TokenKind::Keyword(Keyword::Var)) {
                    VarDeclKind::Var
                } else {
                    VarDeclKind::Val
                };
                p.advance();
                let name = Name::parse(p)?;
                let type_spec = if p.is_symbol(Symbol::Colon) {
                    Some(TypeSpec::parse(p)?)
                } else {
                    None
                };
                let mut span = token.span.merge(name.span);
                if let Some(ts) = &type_spec { span = span.merge(ts.span); }
                Ok(Exp { span, kind: ExpKind::VarDecl { kind: decl_kind, name, type_spec } })
            }
            TokenKind::Symbol(Symbol::LParen) => {
                if Self::can_start_lambda(p)? {
                    let lambda = LambdaExpr::parse(p)?;
                    return Ok(Exp { span: lambda.span, kind: ExpKind::Lambda(lambda) });
                }
                let open = p.advance();
                let inner = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RParen)?;
                Ok(Exp { span: open.span.merge(close.span), kind: ExpKind::Paren(Box::new(inner)) })
            }
            TokenKind::Keyword(Keyword::Const) => {
                if Self::can_start_lambda(p)? {
                    let lambda = LambdaExpr::parse(p)?;
                    return Ok(Exp { span: lambda.span, kind: ExpKind::Lambda(lambda) });
                }
                Err(ParseError {
                    message: "expected lambda after 'const'".to_string(),
                    position: token.span.start,
                })
            }
            _ => Err(ParseError {
                message: format!("unexpected token in expression: {:?}", token.kind),
                position: token.span.start,
            }),
        }
    }

    // ── Lambda lookahead helpers ──────────────────────────────────────────

    fn can_start_lambda(p: &mut Parser) -> Result<bool, ParseError> {
        let checkpoint = p.checkpoint();
        if p.is_keyword(Keyword::Const) { p.advance(); }
        if !p.is_symbol(Symbol::LParen) {
            p.restore(checkpoint);
            return Ok(false);
        }
        p.advance();
        if !p.is_symbol(Symbol::RParen) {
            if !Self::skip_lambda_param(p)? {
                p.restore(checkpoint);
                return Ok(false);
            }
            while p.is_symbol(Symbol::Comma) {
                p.advance();
                if !Self::skip_lambda_param(p)? {
                    p.restore(checkpoint);
                    return Ok(false);
                }
            }
        }
        if !p.is_symbol(Symbol::RParen) {
            p.restore(checkpoint);
            return Ok(false);
        }
        p.advance();
        let has_type = p.is_symbol(Symbol::Colon);
        p.restore(checkpoint);
        Ok(has_type)
    }

    fn skip_lambda_param(p: &mut Parser) -> Result<bool, ParseError> {
        if !matches!(p.current().kind, TokenKind::Identifier(_)) { return Ok(false); }
        p.advance();
        if p.is_symbol(Symbol::Colon) {
            p.advance();
            if !Self::skip_type_name(p) { return Ok(false); }
        }
        Ok(true)
    }

    fn skip_type_name(p: &mut Parser) -> bool {
        if !matches!(p.current().kind, TokenKind::Identifier(_)) { return false; }
        p.advance();
        while p.is_symbol(Symbol::Dot) {
            p.advance();
            if !matches!(p.current().kind, TokenKind::Identifier(_)) { return false; }
            p.advance();
        }
        true
    }
}
