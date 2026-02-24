use super::parsable::Parsable;
use super::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Symbol, TokenKind};
use crate::syntax::ast::{Block, Exp, IfClause, Name, PrefixExp, PrefixExpKind, Stat, StatKind};

impl Parsable for Stat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        // Empty statement
        if p.is_symbol(Symbol::Semi) {
            let token = p.advance();
            return Ok(Stat {
                span: token.span,
                kind: StatKind::Empty,
            });
        }
        // do … end
        if p.is_keyword(Keyword::Do) {
            let token = p.expect_keyword(Keyword::Do)?;
            let block = Block::parse(p)?;
            let end = p.expect_keyword(Keyword::End)?;
            return Ok(Stat {
                span: token.span.merge(end.span),
                kind: StatKind::Do { block },
            });
        }
        // while … do … end
        if p.is_keyword(Keyword::While) {
            let token = p.expect_keyword(Keyword::While)?;
            let cond = Exp::parse(p)?;
            p.expect_keyword(Keyword::Do)?;
            let block = Block::parse(p)?;
            let end = p.expect_keyword(Keyword::End)?;
            return Ok(Stat {
                span: token.span.merge(end.span),
                kind: StatKind::While { cond, block },
            });
        }
        // repeat … until …
        if p.is_keyword(Keyword::Repeat) {
            let token = p.expect_keyword(Keyword::Repeat)?;
            let block = Block::parse(p)?;
            p.expect_keyword(Keyword::Until)?;
            let cond = Exp::parse(p)?;
            return Ok(Stat {
                span: token.span.merge(cond.span),
                kind: StatKind::Repeat { block, cond },
            });
        }
        // if … then … [elseif …] [else …] end
        if p.is_keyword(Keyword::If) {
            let token = p.expect_keyword(Keyword::If)?;
            let cond = Exp::parse(p)?;
            p.expect_keyword(Keyword::Then)?;
            let block = Block::parse(p)?;
            let mut clauses = vec![IfClause {
                span: token.span.merge(block.span),
                cond,
                block,
            }];
            while p.is_keyword(Keyword::ElseIf) {
                let elseif = p.expect_keyword(Keyword::ElseIf)?;
                let cond = Exp::parse(p)?;
                p.expect_keyword(Keyword::Then)?;
                let block = Block::parse(p)?;
                clauses.push(IfClause {
                    span: elseif.span.merge(block.span),
                    cond,
                    block,
                });
            }
            let else_block = if p.is_keyword(Keyword::Else) {
                p.expect_keyword(Keyword::Else)?;
                Some(Block::parse(p)?)
            } else {
                None
            };
            let end = p.expect_keyword(Keyword::End)?;
            return Ok(Stat {
                span: token.span.merge(end.span),
                kind: StatKind::If {
                    clauses,
                    else_block,
                },
            });
        }
        // for (numeric or generic)
        if p.is_keyword(Keyword::For) {
            let token = p.expect_keyword(Keyword::For)?;
            let name = Name::parse(p)?;
            if p.is_symbol(Symbol::Assign) {
                p.advance();
                let start = Exp::parse(p)?;
                p.expect_symbol(Symbol::Comma)?;
                let end = Exp::parse(p)?;
                let step = if p.is_symbol(Symbol::Comma) {
                    p.advance();
                    Some(Exp::parse(p)?)
                } else {
                    None
                };
                p.expect_keyword(Keyword::Do)?;
                let block = Block::parse(p)?;
                let end_kw = p.expect_keyword(Keyword::End)?;
                return Ok(Stat {
                    span: token.span.merge(end_kw.span),
                    kind: StatKind::ForNumeric {
                        name,
                        start,
                        end,
                        step,
                        block,
                    },
                });
            }
            let mut names = vec![name];
            while p.is_symbol(Symbol::Comma) {
                p.advance();
                names.push(Name::parse(p)?);
            }
            p.expect_keyword(Keyword::In)?;
            let exprs = p.parse_exp_list()?;
            p.expect_keyword(Keyword::Do)?;
            let block = Block::parse(p)?;
            let end_kw = p.expect_keyword(Keyword::End)?;
            return Ok(Stat {
                span: token.span.merge(end_kw.span),
                kind: StatKind::ForGeneric {
                    names,
                    exprs,
                    block,
                },
            });
        }
        // break / continue / goto / label
        if p.is_keyword(Keyword::Break) {
            let token = p.expect_keyword(Keyword::Break)?;
            return Ok(Stat {
                span: token.span,
                kind: StatKind::Break,
            });
        }
        if p.is_keyword(Keyword::Continue) {
            let token = p.expect_keyword(Keyword::Continue)?;
            return Ok(Stat {
                span: token.span,
                kind: StatKind::Continue,
            });
        }
        if p.is_keyword(Keyword::Goto) {
            let token = p.expect_keyword(Keyword::Goto)?;
            let label = Name::parse(p)?;
            return Ok(Stat {
                span: token.span.merge(label.span),
                kind: StatKind::Goto { label },
            });
        }
        // ::label::
        if matches!(p.current().kind, TokenKind::Symbol(Symbol::Colon))
            && matches!(p.peek(1).kind, TokenKind::Symbol(Symbol::Colon))
        {
            let start = p.expect_symbol(Symbol::Colon)?;
            p.expect_symbol(Symbol::Colon)?;
            let label = Name::parse(p)?;
            p.expect_symbol(Symbol::Colon)?;
            let end = p.expect_symbol(Symbol::Colon)?;
            return Ok(Stat {
                span: start.span.merge(end.span),
                kind: StatKind::Label { label },
            });
        }
        // assignment or call
        let prefix = PrefixExp::parse(p)?;
        match &prefix.kind {
            PrefixExpKind::Call(call) => {
                if p.is_symbol(Symbol::Assign) || p.is_symbol(Symbol::Comma) {
                    return Err(ParseError {
                        message:
                            "function call cannot be assignment target; use a variable or field"
                                .to_string(),
                        position: prefix.span.start,
                    });
                }
                return Ok(Stat {
                    span: prefix.span,
                    kind: StatKind::Call { call: call.clone() },
                });
            }
            PrefixExpKind::Var(_) => {}
            PrefixExpKind::Paren(_) => return Err(ParseError {
                message:
                    "parenthesized expression cannot start a statement; expected assignment or call"
                        .to_string(),
                position: prefix.span.start,
            }),
        }

        if !p.is_symbol(Symbol::Assign) && !p.is_symbol(Symbol::Comma) {
            return Err(ParseError {
                message: "expected assignment '=' or ',' after variable list".to_string(),
                position: prefix.span.start,
            });
        }

        let first_var = match prefix.kind {
            PrefixExpKind::Var(var) => var,
            _ => {
                return Err(ParseError {
                    message: "invalid assignment target; expected variable, field, or index"
                        .to_string(),
                    position: prefix.span.start,
                });
            }
        };
        let mut vars = vec![first_var];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            let next = PrefixExp::parse(p)?;
            match next.kind {
                PrefixExpKind::Var(var) => vars.push(var),
                _ => {
                    return Err(ParseError {
                        message: "invalid assignment target; expected variable, field, or index"
                            .to_string(),
                        position: next.span.start,
                    });
                }
            }
        }
        let eq = p.expect_symbol(Symbol::Assign)?;
        let exprs = p.parse_exp_list()?;
        let end_span = exprs.last().map(|e| e.span).unwrap_or(eq.span);
        Ok(Stat {
            span: vars
                .last()
                .map(|v| v.span.merge(end_span))
                .unwrap_or(end_span),
            kind: StatKind::Assign { vars, exprs },
        })
    }
}
