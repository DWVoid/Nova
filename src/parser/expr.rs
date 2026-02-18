use super::{Assoc, BlockEnd, ParseError, Parser};
use crate::ast::{Args, ArgsKind, Exp, ExpKind, Field, FieldKey, Initializer, LambdaExpr};
use crate::token::{Keyword, Symbol, TokenKind};

impl Parser {
    pub(super) fn parse_exp_list(&mut self, min_prec: u8) -> Result<Vec<Exp>, ParseError> {
        let mut exprs = vec![self.parse_exp(min_prec)?];
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            exprs.push(self.parse_exp(min_prec)?);
        }
        Ok(exprs)
    }

    pub(super) fn parse_exp(&mut self, min_prec: u8) -> Result<Exp, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let Some((op, prec, assoc)) = self.peek_binop() else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.advance();
            let next_min = if assoc == Assoc::Left { prec + 1 } else { prec };
            let right = self.parse_exp(next_min)?;
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

    fn parse_unary(&mut self) -> Result<Exp, ParseError> {
        if let Some(op) = self.peek_unop() {
            let token = self.advance();
            let expr = self.parse_unary()?;
            let span = token.span.merge(expr.span);
            return Ok(Exp {
                span,
                kind: ExpKind::Unary {
                    op,
                    exp: Box::new(expr),
                },
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Exp, ParseError> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(text) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Number(text),
                })
            }
            TokenKind::StringLiteral(text) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::String(text),
                })
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Nil,
                })
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Bool(true),
                })
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    kind: ExpKind::Bool(false),
                })
            }
            TokenKind::Keyword(Keyword::Const) => {
                if self.can_start_lambda()? {
                    let lambda = self.parse_lambda_expr()?;
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
                if self.can_start_lambda()? {
                    let lambda = self.parse_lambda_expr()?;
                    return Ok(Exp {
                        span: lambda.span,
                        kind: ExpKind::Lambda(lambda),
                    });
                }
                let prefix = self.parse_prefixexp()?;
                Ok(Exp {
                    span: prefix.span,
                    kind: ExpKind::Prefix(prefix),
                })
            }
            TokenKind::Identifier(_) => {
                let prefix = self.parse_prefixexp()?;
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

    fn can_start_lambda(&mut self) -> Result<bool, ParseError> {
        let checkpoint = self.checkpoint();
        if self.is_keyword(Keyword::Const) {
            self.advance();
        }
        if !self.is_symbol(Symbol::LParen) {
            self.restore(checkpoint);
            return Ok(false);
        }
        self.advance();
        if !self.is_symbol(Symbol::RParen) {
            if !self.skip_lambda_param()? {
                self.restore(checkpoint);
                return Ok(false);
            }
            while self.is_symbol(Symbol::Comma) {
                self.advance();
                if !self.skip_lambda_param()? {
                    self.restore(checkpoint);
                    return Ok(false);
                }
            }
        }
        if !self.is_symbol(Symbol::RParen) {
            self.restore(checkpoint);
            return Ok(false);
        }
        self.advance();
        let has_type = self.is_symbol(Symbol::Colon);
        self.restore(checkpoint);
        Ok(has_type)
    }

    fn skip_lambda_param(&mut self) -> Result<bool, ParseError> {
        if !matches!(self.current().kind, TokenKind::Identifier(_)) {
            return Ok(false);
        }
        self.advance();
        if self.is_symbol(Symbol::Colon) {
            self.advance();
            if !self.skip_type_name() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn skip_type_name(&mut self) -> bool {
        if !matches!(self.current().kind, TokenKind::Identifier(_)) {
            return false;
        }
        self.advance();
        while self.is_symbol(Symbol::Dot) {
            self.advance();
            if !matches!(self.current().kind, TokenKind::Identifier(_)) {
                return false;
            }
            self.advance();
        }
        true
    }

    fn parse_lambda_expr(&mut self) -> Result<LambdaExpr, ParseError> {
        let start = self.current().span.start;
        let is_const = if self.is_keyword(Keyword::Const) {
            self.advance();
            true
        } else {
            false
        };
        let params = self.parse_param_list()?;
        let return_type = self.parse_type_spec()?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end = self.expect_keyword(Keyword::End)?;
        let span = crate::token::Span::new(start, end.span.end);
        Ok(LambdaExpr {
            span,
            is_const,
            params,
            return_type,
            block,
        })
    }

    pub(super) fn parse_initializer(&mut self) -> Result<Initializer, ParseError> {
        let open = self.expect_symbol(Symbol::LBrace)?;
        let mut fields = Vec::new();
        while !self.is_symbol(Symbol::RBrace) {
            if self.is_symbol(Symbol::Semi) {
                self.advance();
                continue;
            }
            fields.push(self.parse_field()?);
            if self.is_symbol(Symbol::Comma) || self.is_symbol(Symbol::Semi) {
                self.advance();
            } else {
                break;
            }
        }
        let close = self.expect_symbol(Symbol::RBrace)?;
        let span = open.span.merge(close.span);
        Ok(Initializer { span, fields })
    }

    fn parse_field(&mut self) -> Result<Field, ParseError> {
        if self.is_symbol(Symbol::LBracket) {
            let open = self.advance();
            let key = self.parse_exp(0)?;
            let _close = self.expect_symbol(Symbol::RBracket)?;
            let _eq = self.expect_symbol(Symbol::Assign)?;
            let value = self.parse_exp(0)?;
            let span = open.span.merge(value.span);
            return Ok(Field {
                span,
                key: Some(FieldKey::Exp(key)),
                value,
            });
        }

        if let TokenKind::Identifier(_) = self.current().kind {
            if self.peek_is_symbol(1, Symbol::Assign) {
                let name = self.parse_name()?;
                let _eq = self.expect_symbol(Symbol::Assign)?;
                let value = self.parse_exp(0)?;
                let span = name.span.merge(value.span);
                return Ok(Field {
                    span,
                    key: Some(FieldKey::Name(name)),
                    value,
                });
            }
        }

        let value = self.parse_exp(0)?;
        Ok(Field {
            span: value.span,
            key: None,
            value,
        })
    }

    pub(super) fn parse_args(&mut self) -> Result<Args, ParseError> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Symbol(Symbol::LParen) => {
                let open = self.advance();
                let mut exprs = Vec::new();
                if !self.is_symbol(Symbol::RParen) {
                    exprs = self.parse_exp_list(0)?;
                }
                let close = self.expect_symbol(Symbol::RParen)?;
                Ok(Args {
                    span: open.span.merge(close.span),
                    kind: ArgsKind::ExpList(exprs),
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let init = self.parse_initializer()?;
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