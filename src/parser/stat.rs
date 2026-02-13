use super::{BlockEnd, ParseError, Parser};
use crate::ast::{
    Comments, FuncName, IfClause, LocalAttr, LocalName, Name, PrefixExpKind, RetStat, Stat, StatKind,
};
use crate::token::{Keyword, Span, Symbol, TokenKind};

impl Parser {
    pub(super) fn parse_block(&mut self, end: BlockEnd) -> Result<crate::ast::Block, ParseError> {
        self.detached_stack.push(Vec::new());
        let leading = if self.is_block_end(end) {
            self.take_leading_comments_for_current()
        } else {
            Vec::new()
        };
        let start = self.current().span.start;
        let mut end_pos = start;
        let mut stats = Vec::new();
        let mut ret = None;

        while !self.is_block_end(end) {
            if self.is_keyword(Keyword::Return) {
                let ret_leading = self.take_leading_comments_for_current();
                let retstat = self.parse_retstat(ret_leading)?;
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
        let detached = self.detached_stack.pop().unwrap_or_default();
        Ok(crate::ast::Block {
            span,
            leading_comments: leading,
            stats,
            ret,
            detached_comments: detached,
            trailing_comments: self.take_leading_comments_for_current(),
        })
    }

    pub(super) fn parse_stat(&mut self) -> Result<Stat, ParseError> {
        let leading = self.take_leading_comments_for_current();
        if self.is_symbol(Symbol::Semi) {
            let token = self.advance();
            return Ok(Stat {
                span: token.span,
                leading_comments: leading,
                kind: StatKind::Empty,
                trailing_comments: token.trailing,
            });
        }

        if self.is_keyword(Keyword::Local) {
            return self.parse_local_stat(leading);
        }
        if self.is_keyword(Keyword::Function) {
            return self.parse_function_stat(leading);
        }
        if self.is_keyword(Keyword::Do) {
            return self.parse_do_stat(leading);
        }
        if self.is_keyword(Keyword::While) {
            return self.parse_while_stat(leading);
        }
        if self.is_keyword(Keyword::Repeat) {
            return self.parse_repeat_stat(leading);
        }
        if self.is_keyword(Keyword::If) {
            return self.parse_if_stat(leading);
        }
        if self.is_keyword(Keyword::For) {
            return self.parse_for_stat(leading);
        }
        if self.is_keyword(Keyword::Break) {
            return self.parse_break_stat(leading);
        }
        if self.is_keyword(Keyword::Goto) {
            return self.parse_goto_stat(leading);
        }
        if self.is_label_start() {
            return self.parse_label_stat(leading);
        }

        self.parse_assign_or_call(leading)
    }

    fn parse_retstat(&mut self, leading: Comments) -> Result<RetStat, ParseError> {
        let token = self.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !self.is_block_end(BlockEnd::Nested) && !self.is_symbol(Symbol::Semi) {
            exprs = self.parse_exp_list(0)?;
        }
        let end_span = exprs
            .last()
            .map(|exp| exp.span)
            .unwrap_or(token.span);
        let trailing_comments = exprs
            .last()
            .map(|exp| exp.trailing_comments.clone())
            .unwrap_or_else(|| self.take_trailing_comments());
        Ok(RetStat {
            span: token.span.merge(end_span),
            leading_comments: leading,
            exprs,
            trailing_comments,
        })
    }

    fn parse_local_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Local)?;
        if self.is_keyword(Keyword::Function) {
            let _fn_token = self.expect_keyword(Keyword::Function)?;
            let name = self.parse_name()?;
            let func = self.parse_func_body(token.span.start)?;
            let trailing = self.take_trailing_comments();
            let span = token.span.merge(func.span);
            return Ok(Stat {
                span,
                leading_comments: leading,
                kind: StatKind::LocalFunction { name, func },
                trailing_comments: trailing,
            });
        }

        let mut names = Vec::new();
        names.push(self.parse_local_name()?);
        while self.is_symbol(Symbol::Comma) {
            self.advance();
            names.push(self.parse_local_name()?);
        }

        let mut exprs = Vec::new();
        let mut end_span = names.last().map(|n| n.name.span).unwrap_or(token.span);
        if self.is_symbol(Symbol::Assign) {
            let eq = self.advance();
            exprs = self.parse_exp_list(0)?;
            end_span = exprs.last().map(|e| e.span).unwrap_or(eq.span);
        }

        let trailing_comments = exprs
            .last()
            .map(|exp| exp.trailing_comments.clone())
            .unwrap_or_else(|| self.take_trailing_comments());
        Ok(Stat {
            span: token.span.merge(end_span),
            leading_comments: leading,
            kind: StatKind::LocalAssign { names, exprs },
            trailing_comments,
        })
    }

    fn parse_function_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Function)?;
        let name = self.parse_func_name()?;
        let func = self.parse_func_body(token.span.start)?;
        let trailing = self.take_trailing_comments();
        let span = token.span.merge(func.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::Function { name, func },
            trailing_comments: trailing,
        })
    }

    fn parse_do_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Do)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::Do { block },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_while_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::While)?;
        let cond = self.parse_exp(0)?;
        let _do = self.expect_keyword(Keyword::Do)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let end = self.expect_keyword(Keyword::End)?;
        let span = token.span.merge(end.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::While { cond, block },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_repeat_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Repeat)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let _until = self.expect_keyword(Keyword::Until)?;
        let cond = self.parse_exp(0)?;
        let span = token.span.merge(cond.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::Repeat { block, cond },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_if_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::If)?;
        let cond = self.parse_exp(0)?;
        let _then = self.expect_keyword(Keyword::Then)?;
        let block = self.parse_block(BlockEnd::Nested)?;
        let mut clauses = Vec::new();
        let first_span = token.span.merge(block.span);
        clauses.push(IfClause {
            span: first_span,
            leading_comments: leading,
            cond,
            block,
            trailing_comments: self.take_trailing_comments(),
        });

        while self.is_keyword(Keyword::ElseIf) {
            let else_leading = self.take_leading_comments();
            let elseif = self.expect_keyword(Keyword::ElseIf)?;
            let cond = self.parse_exp(0)?;
            let _then = self.expect_keyword(Keyword::Then)?;
            let block = self.parse_block(BlockEnd::Nested)?;
            let span = elseif.span.merge(block.span);
            clauses.push(IfClause {
                span,
                leading_comments: if else_leading.is_empty() {
                    elseif.leading
                } else {
                    else_leading
                },
                cond,
                block,
                trailing_comments: self.take_trailing_comments(),
            });
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
            leading_comments: clauses
                .first()
                .map(|c| c.leading_comments.clone())
                .unwrap_or_else(Vec::new),
            kind: StatKind::If { clauses, else_block },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_for_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
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
                leading_comments: leading,
                kind: StatKind::ForNumeric {
                    name,
                    start,
                    end,
                    step,
                    block,
                },
                trailing_comments: self.take_trailing_comments(),
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
            leading_comments: leading,
            kind: StatKind::ForGeneric { names, exprs, block },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_break_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Break)?;
        Ok(Stat {
            span: token.span,
            leading_comments: leading,
            kind: StatKind::Break,
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_goto_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let token = self.expect_keyword(Keyword::Goto)?;
        let label = self.parse_name()?;
        let span = token.span.merge(label.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::Goto { label },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_label_stat(&mut self, leading: Comments) -> Result<Stat, ParseError> {
        let start = self.expect_symbol(Symbol::Colon)?;
        let _second = self.expect_symbol(Symbol::Colon)?;
        let label = self.parse_name()?;
        let _third = self.expect_symbol(Symbol::Colon)?;
        let end = self.expect_symbol(Symbol::Colon)?;
        let span = start.span.merge(end.span);
        Ok(Stat {
            span,
            leading_comments: leading,
            kind: StatKind::Label { label },
            trailing_comments: self.take_trailing_comments(),
        })
    }

    fn parse_assign_or_call(&mut self, leading: Comments) -> Result<Stat, ParseError> {
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
                    leading_comments: leading,
                    kind: StatKind::Call { call: call.clone() },
                    trailing_comments: prefix.trailing_comments.clone(),
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
        let trailing_comments = exprs
            .last()
            .map(|e| e.trailing_comments.clone())
            .unwrap_or_else(Vec::new);
        Ok(Stat {
            span: vars
                .last()
                .map(|v| v.span.merge(end_span))
                .unwrap_or(end_span),
            leading_comments: leading,
            kind: StatKind::Assign { vars, exprs },
            trailing_comments,
        })
    }

    fn parse_local_name(&mut self) -> Result<LocalName, ParseError> {
        let name = self.parse_name()?;
        let attr = if self.is_symbol(Symbol::Less) {
            self.advance();
            let attr_name = self.parse_name()?;
            let _gt = self.expect_symbol(Symbol::Greater)?;
            match attr_name.value.as_str() {
                "const" => Some(LocalAttr::Const),
                "close" => Some(LocalAttr::Close),
                _ => {
                    return Err(ParseError {
                        message: format!("unknown local attribute: {}", attr_name.value),
                        position: attr_name.span.start,
                    })
                }
            }
        } else {
            None
        };
        Ok(LocalName { name, attr })
    }

    fn parse_func_name(&mut self) -> Result<FuncName, ParseError> {
        let mut names = Vec::new();
        names.push(self.parse_name()?);
        while self.is_symbol(Symbol::Dot) {
            self.advance();
            names.push(self.parse_name()?);
        }
        let method = if self.is_symbol(Symbol::Colon) {
            self.advance();
            Some(self.parse_name()?)
        } else {
            None
        };

        let mut span = names[0].span;
        for name in &names[1..] {
            span = span.merge(name.span);
        }
        if let Some(method) = &method {
            span = span.merge(method.span);
        }
        Ok(FuncName {
            span,
            leading_comments: names[0].leading_comments.clone(),
            names,
            method,
            trailing_comments: Vec::new(),
        })
    }

    pub(crate) fn parse_name(&mut self) -> Result<Name, ParseError> {
        let token = self.current().clone();
        if let TokenKind::Identifier(value) = token.kind {
            self.advance();
            return Ok(Name {
                value,
                span: token.span,
                leading_comments: token.leading,
                trailing_comments: token.trailing,
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
