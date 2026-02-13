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
                    kind: VarKind::Name(name),
                };
                PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                }
            }
            TokenKind::Symbol(Symbol::LParen) => {
                let open = self.advance();
                let expr = self.parse_exp(0)?;
                let close = self.expect_symbol(Symbol::RParen)?;
                PrefixExp {
                    span: open.span.merge(close.span),
                    kind: PrefixExpKind::Paren(Box::new(expr)),
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
                    kind: VarKind::Field {
                        prefix: Box::new(prefix),
                        name,
                    },
                };
                prefix = PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
                };
                continue;
            }

            if self.is_symbol(Symbol::LBracket) {
                let _open = self.advance();
                let index = self.parse_exp(0)?;
                let close = self.expect_symbol(Symbol::RBracket)?;
                let var = Var {
                    span: prefix.span.merge(close.span),
                    kind: VarKind::Index {
                        prefix: Box::new(prefix),
                        index: Box::new(index),
                    },
                };
                prefix = PrefixExp {
                    span: var.span,
                    kind: PrefixExpKind::Var(var),
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
                    prefix: Box::new(prefix),
                    method: Some(method),
                    args,
                };
                prefix = PrefixExp {
                    span: call.span,
                    kind: PrefixExpKind::Call(call),
                };
                continue;
            }

            if self.is_args_start() {
                let args = self.parse_args()?;
                let span = prefix.span.merge(args.span);
                let call = FunctionCall {
                    span,
                    prefix: Box::new(prefix),
                    method: None,
                    args,
                };
                prefix = PrefixExp {
                    span: call.span,
                    kind: PrefixExpKind::Call(call),
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