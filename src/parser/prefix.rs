use super::{ParseError, Parser};
use crate::ast::{FunctionCall, PrefixExp, PrefixExpKind, Var, VarKind};
use crate::token::{Symbol, TokenKind};

impl Parser {
    pub(crate) fn parse_prefixexp(&mut self) -> Result<PrefixExp, ParseError> {
        let base = match self.current().kind.clone() {
            TokenKind::Identifier(_) => {
                let name = self.parse_name()?;
                let var = Var {
                    span: name.span,
                    leading_comments: vec![],
                    kind: VarKind::Name(name),
                    trailing_comments: vec![],
                };
                PrefixExp {
                    span: var.span,
                    leading_comments: vec![],
                    kind: PrefixExpKind::Var(var),
                    trailing_comments: vec![],
                }
            }
            TokenKind::Symbol(Symbol::LParen) => {
                let open = self.advance();
                let expr = self.parse_exp(0)?;
                let close = self.expect_symbol(Symbol::RParen)?;
                PrefixExp {
                    span: open.span.merge(close.span),
                    leading_comments: open.leading,
                    kind: PrefixExpKind::Paren(Box::new(expr)),
                    trailing_comments: close.trailing,
                }
            }
            _ => {
                return Err(ParseError {
                    message: "expected name or '('".to_string(),
                    position: self.current().span.start,
                })
            }
        };

        self.parse_prefix_suffixes(base)
    }

    fn parse_prefix_suffixes(&mut self, mut prefix: PrefixExp) -> Result<PrefixExp, ParseError> {
        loop {
            if self.is_symbol(Symbol::Dot) {
                self.advance();
                let name = self.parse_name()?;
                let var = Var {
                    span: prefix.span.merge(name.span),
                    leading_comments: vec![],
                    kind: VarKind::Field {
                        prefix: Box::new(prefix),
                        name,
                    },
                    trailing_comments: self.take_trailing_comments(),
                };
                prefix = PrefixExp {
                    span: var.span,
                    leading_comments: vec![],
                    kind: PrefixExpKind::Var(var),
                    trailing_comments: vec![],
                };
                continue;
            }

            if self.is_symbol(Symbol::LBracket) {
                let _open = self.advance();
                let index = self.parse_exp(0)?;
                let close = self.expect_symbol(Symbol::RBracket)?;
                let var = Var {
                    span: prefix.span.merge(close.span),
                    leading_comments: vec![],
                    kind: VarKind::Index {
                        prefix: Box::new(prefix),
                        index: Box::new(index),
                    },
                    trailing_comments: self.take_trailing_comments(),
                };
                prefix = PrefixExp {
                    span: var.span,
                    leading_comments: vec![],
                    kind: PrefixExpKind::Var(var),
                    trailing_comments: vec![],
                };
                continue;
            }

            if self.is_symbol(Symbol::Colon) {
                self.advance();
                let method = self.parse_name()?;
                let args = self.parse_args()?;
                let span = prefix.span.merge(args.span);
                let call = FunctionCall {
                    span,
                    leading_comments: vec![],
                    prefix: Box::new(prefix),
                    method: Some(method),
                    args,
                    trailing_comments: self.take_trailing_comments(),
                };
                prefix = PrefixExp {
                    span: call.span,
                    leading_comments: vec![],
                    kind: PrefixExpKind::Call(call),
                    trailing_comments: vec![],
                };
                continue;
            }

            if self.is_args_start() {
                let args = self.parse_args()?;
                let span = prefix.span.merge(args.span);
                let call = FunctionCall {
                    span,
                    leading_comments: vec![],
                    prefix: Box::new(prefix),
                    method: None,
                    args,
                    trailing_comments: self.take_trailing_comments(),
                };
                prefix = PrefixExp {
                    span: call.span,
                    leading_comments: vec![],
                    kind: PrefixExpKind::Call(call),
                    trailing_comments: vec![],
                };
                continue;
            }

            break;
        }

        Ok(prefix)
    }

    pub(crate) fn is_args_start(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Symbol(Symbol::LParen)
                | TokenKind::Symbol(Symbol::LBrace)
                | TokenKind::StringLiteral(_)
        )
    }
}