use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{Assoc, ParseError, Parser};
use crate::lexical::{Keyword, Symbol, TokenKind};
use super::lambda_expr::LambdaExpr;
use super::ops::{BinOp, UnOp};
use super::prefix_exp::PrefixExp;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ExpKind {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Prefix(PrefixExp),
    Lambda(LambdaExpr),
    Unary { op: UnOp, exp: Box<Exp> },
    Binary { op: BinOp, left: Box<Exp>, right: Box<Exp> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Exp {
    pub span: Span,
    pub kind: ExpKind,
}

impl Parsable for Exp {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        Self::parse_exp_prec(p, 0)
    }
}

impl Exp {
    fn parse_exp_prec(p: &mut Parser, min_prec: u8) -> Result<Exp, ParseError> {
        let mut left = Self::parse_unary(p)?;
        loop {
            let Some((op, prec, assoc)) = p.peek_binop() else {
                break;
            };
            if prec < min_prec {
                break;
            }
            p.advance();
            let next_min = if assoc == Assoc::Left { prec + 1 } else { prec };
            let right = Self::parse_exp_prec(p, next_min)?;
            let span = left.span.merge(right.span);
            left = Exp {
                span,
                kind: ExpKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }
        Ok(left)
    }

    fn parse_unary(p: &mut Parser) -> Result<Exp, ParseError> {
        if let Some(op) = p.peek_unop() {
            let token = p.advance();
            let expr = Self::parse_unary(p)?;
            let span = token.span.merge(expr.span);
            return Ok(Exp {
                span,
                kind: ExpKind::Unary {
                    op,
                    exp: Box::new(expr),
                },
            });
        }
        Self::parse_primary(p)
    }

    fn parse_primary(p: &mut Parser) -> Result<Exp, ParseError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Number(text) => {
                p.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Number(text),
                })
            }
            TokenKind::StringLiteral(text) => {
                p.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::String(text),
                })
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
            TokenKind::Keyword(Keyword::Const) => {
                if Self::can_start_lambda(p)? {
                    let lambda = LambdaExpr::parse(p)?;
                    return Ok(Exp {
                        span: lambda.span,
                        kind: ExpKind::Lambda(lambda),
                    });
                }
                Err(ParseError {
                    message: "expected lambda after 'const'".to_string(),
                    position: token.span.start,
                })
            }
            TokenKind::Symbol(Symbol::LParen) => {
                if Self::can_start_lambda(p)? {
                    let lambda = LambdaExpr::parse(p)?;
                    return Ok(Exp {
                        span: lambda.span,
                        kind: ExpKind::Lambda(lambda),
                    });
                }
                let prefix = PrefixExp::parse(p)?;
                Ok(Exp { span: prefix.span, kind: ExpKind::Prefix(prefix) })
            }
            TokenKind::Identifier(_) => {
                let prefix = PrefixExp::parse(p)?;
                Ok(Exp { span: prefix.span, kind: ExpKind::Prefix(prefix) })
            }
            _ => Err(ParseError {
                message: format!("unexpected token in expression: {:?}", token.kind),
                position: token.span.start,
            }),
        }
    }

    fn can_start_lambda(p: &mut Parser) -> Result<bool, ParseError> {
        let checkpoint = p.checkpoint();
        if p.is_keyword(Keyword::Const) {
            p.advance();
        }
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
        if !matches!(p.current().kind, TokenKind::Identifier(_)) {
            return Ok(false);
        }
        p.advance();
        if p.is_symbol(Symbol::Colon) {
            p.advance();
            if !Self::skip_type_name(p) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn skip_type_name(p: &mut Parser) -> bool {
        if !matches!(p.current().kind, TokenKind::Identifier(_)) {
            return false;
        }
        p.advance();
        while p.is_symbol(Symbol::Dot) {
            p.advance();
            if !matches!(p.current().kind, TokenKind::Identifier(_)) {
                return false;
            }
            p.advance();
        }
        true
    }
}
