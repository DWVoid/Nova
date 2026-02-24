use super::parsable::Parsable;
use super::parser::{Assoc, ParseError, Parser};
use crate::lexical::{Keyword, Span, Symbol, TokenKind};
use crate::syntax::ast::{Args, ArgsKind, Block, Exp, ExpKind, Field, FieldKey, FunctionCall, Initializer, LambdaExpr, Name, PrefixExp, PrefixExpKind, TypeSpec, Var, VarDeclKind, VarKind};

impl Parser {
    pub(super) fn parse_exp_list(&mut self) -> Result<Vec<Exp>, ParseError> {
        let mut exprs = vec![Exp::parse(self)?];
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            exprs.push(Exp::parse(self)?);
        }
        Ok(exprs)
    }
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
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Nil,
                })
            }
            TokenKind::Keyword(Keyword::True) => {
                p.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Bool(true),
                })
            }
            TokenKind::Keyword(Keyword::False) => {
                p.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Bool(false),
                })
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
                Ok(Exp {
                    span: prefix.span,
                    kind: ExpKind::Prefix(prefix),
                })
            }
            TokenKind::Identifier(_) => {
                let prefix = PrefixExp::parse(p)?;
                Ok(Exp {
                    span: prefix.span,
                    kind: ExpKind::Prefix(prefix),
                })
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

impl Parsable for LambdaExpr {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.current().span.start;
        let is_const = if p.is_keyword(Keyword::Const) {
            p.advance();
            true
        } else {
            false
        };
        let params = p.parse_param_list()?;
        let return_type = TypeSpec::parse(p)?;
        let block = Block::parse(p)?;
        let end = p.expect_keyword(Keyword::End)?;
        let span = Span::new(start, end.span.end);
        Ok(LambdaExpr {
            span,
            is_const,
            params,
            return_type,
            block,
        })
    }
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

impl Parsable for Var {
    /// Parse a single variable *base* (name or `var`/`val` declaration).
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

impl Parsable for FunctionCall {
    /// Parse a function call given that `prefix` has already been parsed.
    /// This impl parses a *fresh* prefix and at least one call suffix.
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let prefix_exp = PrefixExp::parse(p)?;
        match prefix_exp.kind {
            PrefixExpKind::Call(call) => Ok(call),
            _ => Err(ParseError {
                message: "expected function call".to_string(),
                position: prefix_exp.span.start,
            }),
        }
    }
}

impl Parsable for Initializer {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let open = p.expect_symbol(Symbol::LBrace)?;
        let mut fields = Vec::new();
        while !p.is_symbol(Symbol::RBrace) {
            if p.is_symbol(Symbol::Semi) {
                p.advance();
                continue;
            }
            fields.push(Field::parse(p)?);
            if p.is_symbol(Symbol::Comma) || p.is_symbol(Symbol::Semi) {
                p.advance();
            } else {
                break;
            }
        }
        let close = p.expect_symbol(Symbol::RBrace)?;
        let span = open.span.merge(close.span);
        Ok(Initializer { span, fields })
    }
}

impl Parsable for Field {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        if p.is_symbol(Symbol::LBracket) {
            let open = p.advance();
            let key = Exp::parse(p)?;
            p.expect_symbol(Symbol::RBracket)?;
            p.expect_symbol(Symbol::Assign)?;
            let value = Exp::parse(p)?;
            let span = open.span.merge(value.span);
            return Ok(Field {
                span,
                key: Some(FieldKey::Exp(key)),
                value,
            });
        }
        if let TokenKind::Identifier(_) = p.current().kind {
            if p.peek_is_symbol(1, Symbol::Assign) {
                let name = Name::parse(p)?;
                p.expect_symbol(Symbol::Assign)?;
                let value = Exp::parse(p)?;
                let span = name.span.merge(value.span);
                return Ok(Field {
                    span,
                    key: Some(FieldKey::Name(name)),
                    value,
                });
            }
        }
        let value = Exp::parse(p)?;
        Ok(Field {
            span: value.span,
            key: None,
            value,
        })
    }
}

impl Parsable for Args {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Symbol(Symbol::LParen) => {
                let open = p.advance();
                let mut exprs = Vec::new();
                if !p.is_symbol(Symbol::RParen) {
                    exprs = p.parse_exp_list()?;
                }
                let close = p.expect_symbol(Symbol::RParen)?;
                Ok(Args {
                    span: open.span.merge(close.span),
                    kind: ArgsKind::ExpList(exprs),
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let init = Initializer::parse(p)?;
                Ok(Args {
                    span: init.span,
                    kind: ArgsKind::Initializer(init),
                })
            }
            _ => Err(ParseError {
                message: "expected call arguments".to_string(),
                position: token.span.start,
            }),
        }
    }
}
