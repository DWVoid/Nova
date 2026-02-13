use super::{Assoc, BlockEnd, ParseError, Parser};
use crate::ast::{Args, ArgsKind, Exp, ExpKind, Field, FieldKey, FuncBody, Name, TableConstructor};
use crate::token::{Keyword, Position, Symbol, TokenKind};

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
            let leading = left.leading_comments.clone();
            let trailing = right.trailing_comments.clone();
            left = Exp {
                span,
                leading_comments: leading,
                kind: ExpKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                trailing_comments: trailing,
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
                leading_comments: token.leading,
                kind: ExpKind::Unary {
                    op,
                    exp: Box::new(expr),
                },
                trailing_comments: token.trailing,
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
                    leading_comments: token.leading,
                    kind: ExpKind::Number(text),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::StringLiteral(text) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ExpKind::String(text),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ExpKind::Nil,
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ExpKind::Bool(true),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Exp {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ExpKind::Bool(false),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Symbol(Symbol::DotDotDot) => {
                if !self.vararg_allowed {
                    return Err(ParseError {
                        message: "vararg usage outside vararg function".to_string(),
                        position: token.span.start,
                    });
                }
                self.advance();
                Ok(Exp {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ExpKind::Vararg,
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let table = self.parse_table_constructor()?;
                Ok(Exp {
                    span: table.span,
                    leading_comments: vec![],
                    kind: ExpKind::Table(table),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Keyword(Keyword::Function) => {
                let start = self.advance();
                let func = self.parse_func_body(start.span.start)?;
                let span = start.span.merge(func.span);
                Ok(Exp {
                    span,
                    leading_comments: start.leading,
                    kind: ExpKind::FuncDef(func),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            TokenKind::Identifier(_) | TokenKind::Symbol(Symbol::LParen) => {
                let prefix = self.parse_prefixexp()?;
                Ok(Exp {
                    span: prefix.span,
                    leading_comments: vec![],
                    kind: ExpKind::Prefix(prefix),
                    trailing_comments: self.take_trailing_comments(),
                })
            }
            _ => Err(ParseError {
                message: format!("unexpected token in expression: {:?}", token.kind),
                position: token.span.start,
            }),
        }
    }

    pub(super) fn parse_func_body(&mut self, start: Position) -> Result<FuncBody, ParseError> {
        let open = self.expect_symbol(Symbol::LParen)?;
        let (params, is_vararg) = self.parse_parlist()?;
        let _close = self.expect_symbol(Symbol::RParen)?;

        let prev_vararg = self.vararg_allowed;
        self.vararg_allowed = is_vararg;
        let block = self.parse_block(BlockEnd::Nested)?;
        self.vararg_allowed = prev_vararg;

        let end = self.expect_keyword(Keyword::End)?;
        let span = crate::token::Span::new(start, end.span.end);
        Ok(FuncBody {
            span,
            leading_comments: open.leading,
            params,
            is_vararg,
            block,
            trailing_comments: end.trailing,
        })
    }

    fn parse_parlist(&mut self) -> Result<(Vec<Name>, bool), ParseError> {
        if self.is_symbol(Symbol::RParen) {
            return Ok((Vec::new(), false));
        }

        if self.is_symbol(Symbol::DotDotDot) {
            let _dots = self.advance();
            return Ok((Vec::new(), true));
        }

        let mut params = Vec::new();
        let mut is_vararg = false;
        params.push(self.parse_name()?);
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            if self.is_symbol(Symbol::DotDotDot) {
                self.advance();
                is_vararg = true;
                break;
            }
            params.push(self.parse_name()?);
        }
        Ok((params, is_vararg))
    }

    pub(super) fn parse_table_constructor(&mut self) -> Result<TableConstructor, ParseError> {
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
        Ok(TableConstructor {
            span,
            leading_comments: open.leading,
            fields,
            trailing_comments: self.take_trailing_comments(),
        })
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
                leading_comments: open.leading,
                key: Some(FieldKey::Exp(key)),
                value,
                trailing_comments: self.take_trailing_comments(),
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
                    leading_comments: vec![],
                    key: Some(FieldKey::Name(name)),
                    value,
                    trailing_comments: self.take_trailing_comments(),
                });
            }
        }

        let value = self.parse_exp(0)?;
        Ok(Field {
            span: value.span,
            leading_comments: vec![],
            key: None,
            value,
            trailing_comments: self.take_trailing_comments(),
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
                    leading_comments: open.leading,
                    kind: ArgsKind::ExpList(exprs),
                    trailing_comments: close.trailing,
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let table = self.parse_table_constructor()?;
                Ok(Args {
                    span: table.span,
                    leading_comments: table.leading_comments.clone(),
                    kind: ArgsKind::Table(table),
                    trailing_comments: vec![],
                })
            }
            TokenKind::StringLiteral(text) => {
                self.advance();
                Ok(Args {
                    span: token.span,
                    leading_comments: token.leading,
                    kind: ArgsKind::String(text),
                    trailing_comments: token.trailing,
                })
            }
            _ => Err(ParseError {
                message: "expected function call arguments".to_string(),
                position: token.span.start,
            }),
        }
    }
}