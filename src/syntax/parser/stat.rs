use super::{BlockEnd, ParseError, Parser};
use crate::syntax::ast::{Block, IfClause, Name, PrefixExpKind, RetStat, Stat, StatKind};
use crate::lexical::token::{Keyword, Span, Symbol, TokenKind};

impl Parser {
    pub(super) fn parse_block(&mut self, end: BlockEnd) -> Result<Block, ParseError> {
        let start = self.current().span.start;
        let mut end_pos = start;
        let mut stats = Vec::new();
        let mut ret = None;

        while !self.is_block_end(end) {
            if self.is_keyword(Keyword::Return) {
                let retstat = self.parse_retstat()?;
                end_pos = retstat.span.end;
                ret = Some(retstat);
                if self.is_symbol(Symbol::Semi) {
                    let semi = self.advance();
                    end_pos = semi.span.end;
                }
                break;
            }
            let stat = self.parse_stat()?;
            end_pos = stat.span.end;
            stats.push(stat);
        }

        let span = Span::new(start, end_pos);
        Ok(Block { span, stats, ret })
    }

    pub(super) fn parse_stat(&mut self) -> Result<Stat, ParseError> {
        if self.is_symbol(Symbol::Semi) {
            let token = self.advance();
            return Ok(Stat {
                span: token.span,
                kind: StatKind::Empty,
            });
        }

        if self.is_keyword(Keyword::Do) {
            return self.parse_do_stat();
        }
        if self.is_keyword(Keyword::While) {
            return self.parse_while_stat();
        }
        if self.is_keyword(Keyword::Repeat) {
            return self.parse_repeat_stat();
        }
        if self.is_keyword(Keyword::If) {
            return self.parse_if_stat();
        }
        if self.is_keyword(Keyword::For) {
            return self.parse_for_stat();
        }
        if self.is_keyword(Keyword::Break) {
            return self.parse_break_stat();
        }
        if self.is_keyword(Keyword::Continue) {
            return self.parse_continue_stat();
        }
        if self.is_keyword(Keyword::Goto) {
            return self.parse_goto_stat();
        }
        if self.is_label_start() {
            return self.parse_label_stat();
        }

        self.parse_assign_or_call()
    }

    fn parse_retstat(&mut self) -> Result<RetStat, ParseError> {
        let token = self.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !self.is_block_end(BlockEnd::Nested) && !self.is_symbol(Symbol::Semi) {
            exprs = self.parse_exp_list(0)?;
        }
        let end_span = exprs
            .last()
            .map(|exp| exp.span)
            .unwrap_or(token.span);
        Ok(RetStat {
            span: token.span.merge(end_span),
            exprs,
        })
    }

    fn parse_break_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Break)?;
        Ok(Stat {
            span: token.span,
            kind: StatKind::Break,
        })
    }

    fn parse_continue_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Continue)?;
        Ok(Stat {
            span: token.span,
            kind: StatKind::Continue,
        })
    }

    fn parse_do_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Do)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end.span);
        Ok(Stat {
            span,
            kind: StatKind::Do { block },
        })
    }

    fn parse_while_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::While)?;
        let cond = self.parse_exp(0)?;
        let _do = self.expect_keyword(Keyword::Do)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end.span);
        Ok(Stat {
            span,
            kind: StatKind::While { cond, block },
        })
    }

    fn parse_repeat_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Repeat)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let _until = self.expect_keyword(Keyword::Until)?;
        let cond = self.parse_exp(0)?;
        let span = token.span.merge(cond.span);
        Ok(Stat {
            span,
            kind: StatKind::Repeat { block, cond },
        })
    }

    fn parse_if_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::If)?;
        let cond = self.parse_exp(0)?;
        let _then = self.expect_keyword(Keyword::Then)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let mut clauses = Vec::new();
        let first_span = token.span.merge(block.span);
        clauses.push(IfClause {
            span: first_span,
            cond,
            block,
        });

        while self.is_keyword(Keyword::ElseIf) {
            let elseif = self.expect_keyword(Keyword::ElseIf)?;
            let cond = self.parse_exp(0)?;
            let _then = self.expect_keyword(Keyword::Then)?;
            let block = self.parse_block(BlockEnd::Nested)?;
            let span = elseif.span.merge(block.span);
            clauses.push(IfClause { span, cond, block });
        }

        let else_block = if self.is_keyword(Keyword::Else) {
            let _else = self.expect_keyword(Keyword::Else)?;
            Some(self.parse_block(BlockEnd::Nested)?)
        } else {
            None
        };
        let end = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end.span);
        Ok(Stat {
            span,
            kind: StatKind::If { clauses, else_block },
        })
    }

    fn parse_for_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::For)?;
        let name = self.parse_name()?;
        if self.is_symbol(Symbol::Assign) {
            self.advance();
            let start = self.parse_exp(0)?;
            self.expect_symbol(Symbol::Comma)?;
            let end = self.parse_exp(0)?;
            let step = if self.is_symbol(Symbol::Comma) {
                self.advance();
                Some(self.parse_exp(0)?)
            } else {
                None
            };
            let _do = self.expect_keyword(Keyword::Do)?;
            let block = self.parse_block(BlockEnd::Nested)?;
            let end_kw = self.expect_keyword(Keyword::End)?;
            let span = token.span.merge(end_kw.span);
            return Ok(Stat {
                span,
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
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            names.push(self.parse_name()?);
        }
        self.expect_keyword(Keyword::In)?;
        let exprs = self.parse_exp_list(0)?;
        let _do = self.expect_keyword(Keyword::Do)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end_kw = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end_kw.span);
        Ok(Stat {
            span,
            kind: StatKind::ForGeneric { names, exprs, block },
        })
    }

    fn parse_assign_or_call(&mut self) -> Result<Stat, ParseError> {
        let prefix = self.parse_prefixexp()?;
        match &prefix.kind {
            PrefixExpKind::Call(call) => {
                if self.is_symbol(Symbol::Assign) || self.is_symbol(Symbol::Comma) {
                    return Err(ParseError {
                        message: "function call cannot be assignment target; use a variable or field".to_string(),
                        position: prefix.span.start,
                    });
                }
                return Ok(Stat {
                    span: prefix.span,
                    kind: StatKind::Call { call: call.clone() },
                });
            }
            PrefixExpKind::Var(_) => {}
            PrefixExpKind::Paren(_) => {
                return Err(ParseError {
                    message: "parenthesized expression cannot start a statement; expected assignment or call".to_string(),
                    position: prefix.span.start,
                })
            }
        }

        if !self.is_symbol(Symbol::Assign) && !self.is_symbol(Symbol::Comma) {
            return Err(ParseError {
                message: "expected assignment '=' or ',' after variable list".to_string(),
                position: prefix.span.start,
            });
        }

        let first_var = match prefix.kind {
            PrefixExpKind::Var(var) => var,
            _ => {
                return Err(ParseError {
                    message: "invalid assignment target; expected variable, field, or index".to_string(),
                    position: prefix.span.start,
                })
            }
        };
        let mut vars = vec![first_var];
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            let next = self.parse_prefixexp()?;
            match next.kind {
                PrefixExpKind::Var(var) => vars.push(var),
                _ => {
                    return Err(ParseError {
                        message: "invalid assignment target; expected variable, field, or index".to_string(),
                        position: next.span.start,
                    })
                }
            }
        }
        let eq = self.expect_symbol(Symbol::Assign)?;
        let exprs = self.parse_exp_list(0)?;
        let end_span = exprs.last().map(|e| e.span).unwrap_or(eq.span);
        Ok(Stat {
            span: vars
                .last()
                .map(|v| v.span.merge(end_span))
                .unwrap_or(end_span),
            kind: StatKind::Assign { vars, exprs },
        })
    }

    fn parse_goto_stat(&mut self) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Goto)?;
        let label = self.parse_name()?;
        let span = token.span.merge(label.span);
        Ok(Stat {
            span,
            kind: StatKind::Goto { label },
        })
    }

    fn parse_label_stat(&mut self) -> Result<Stat, ParseError> {
        let start = self.expect_symbol(Symbol::Colon)?;
        let _second = self.expect_symbol(Symbol::Colon)?;
        let label = self.parse_name()?;
        let _third = self.expect_symbol(Symbol::Colon)?;
        let end = self.expect_symbol(Symbol::Colon)?;
        let span = start.span.merge(end.span);
        Ok(Stat {
            span,
            kind: StatKind::Label { label },
        })
    }

    pub(crate) fn parse_name(&mut self) -> Result<Name, ParseError> {
        let token = self.current().clone();
        if let TokenKind::Identifier(value) = token.kind {
            self.advance();
            return Ok(Name {
                value,
                span: token.span,
            });
        }
        Err(ParseError {
            message: "expected identifier".to_string(),
            position: token.span.start,
        })
    }

    fn is_label_start(&self) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(Symbol::Colon))
            && matches!(self.peek(1).kind, TokenKind::Symbol(Symbol::Colon))
    }
}