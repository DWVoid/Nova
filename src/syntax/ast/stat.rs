use serde::Serialize;
use crate::lexical::{Keyword, Symbol, TokenKind};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::stat_assign::StatAssign;
use super::stat_call::StatCall;
use super::stat_do::StatDo;
use super::stat_empty::StatEmpty;
use super::stat_for::{StatForGeneric, StatForNumeric};
use super::stat_if::StatIf;
use super::stat_jump::{StatBreak, StatContinue};
use super::stat_label::{StatGoto, StatLabel, is_label_start};
use super::stat_repeat::StatRepeat;
use super::stat_while::StatWhile;
use super::prefix_exp::{PrefixExp, PrefixExpKind};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Stat {
    Empty(StatEmpty),
    Do(StatDo),
    While(StatWhile),
    Repeat(StatRepeat),
    If(StatIf),
    ForNumeric(StatForNumeric),
    ForGeneric(StatForGeneric),
    Break(StatBreak),
    Continue(StatContinue),
    Goto(StatGoto),
    Label(StatLabel),
    Call(StatCall),
    Assign(StatAssign),
}

impl Stat {
    pub fn span(&self) -> crate::lexical::Span {
        match self {
            Stat::Empty(s) => s.span,
            Stat::Do(s) => s.span,
            Stat::While(s) => s.span,
            Stat::Repeat(s) => s.span,
            Stat::If(s) => s.span,
            Stat::ForNumeric(s) => s.span,
            Stat::ForGeneric(s) => s.span,
            Stat::Break(s) => s.span,
            Stat::Continue(s) => s.span,
            Stat::Goto(s) => s.span,
            Stat::Label(s) => s.span,
            Stat::Call(s) => s.span,
            Stat::Assign(s) => s.span,
        }
    }
}

impl Parsable for Stat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        if p.is_symbol(Symbol::Semi) {
            return Ok(Stat::Empty(StatEmpty::parse(p)?));
        }
        if p.is_keyword(Keyword::Do) {
            return Ok(Stat::Do(StatDo::parse(p)?));
        }
        if p.is_keyword(Keyword::While) {
            return Ok(Stat::While(StatWhile::parse(p)?));
        }
        if p.is_keyword(Keyword::Repeat) {
            return Ok(Stat::Repeat(StatRepeat::parse(p)?));
        }
        if p.is_keyword(Keyword::If) {
            return Ok(Stat::If(StatIf::parse(p)?));
        }
        if p.is_keyword(Keyword::For) {
            // Peek: `for name =` → numeric; otherwise generic
            return Ok(if matches!(p.peek(2).kind, TokenKind::Symbol(Symbol::Assign)) {
                Stat::ForNumeric(StatForNumeric::parse(p)?)
            } else {
                Stat::ForGeneric(StatForGeneric::parse(p)?)
            });
        }
        if p.is_keyword(Keyword::Break) {
            return Ok(Stat::Break(StatBreak::parse(p)?));
        }
        if p.is_keyword(Keyword::Continue) {
            return Ok(Stat::Continue(StatContinue::parse(p)?));
        }
        if p.is_keyword(Keyword::Goto) {
            return Ok(Stat::Goto(StatGoto::parse(p)?));
        }
        if is_label_start(p) {
            return Ok(Stat::Label(StatLabel::parse(p)?));
        }
        // assignment or call — parse prefix expression first
        let prefix = PrefixExp::parse(p)?;
        match &prefix.kind {
            PrefixExpKind::Paren(_) => {
                return Err(ParseError {
                    message: "parenthesized expression cannot start a statement; expected assignment or call"
                        .to_string(),
                    position: prefix.span.start,
                });
            }
            PrefixExpKind::Call(_) => {
                if p.is_symbol(Symbol::Assign) || p.is_symbol(Symbol::Comma) {
                    return Err(ParseError {
                        message: "function call cannot be assignment target; use a variable or field"
                            .to_string(),
                        position: prefix.span.start,
                    });
                }
                match prefix.kind {
                    PrefixExpKind::Call(call) => {
                        return Ok(Stat::Call(StatCall { span: prefix.span, call }));
                    }
                    _ => unreachable!(),
                }
            }
            PrefixExpKind::Var(_) => {}
        }
        // Must be assignment
        if !p.is_symbol(Symbol::Assign) && !p.is_symbol(Symbol::Comma) {
            return Err(ParseError {
                message: "expected assignment '=' or ',' after variable".to_string(),
                position: prefix.span.start,
            });
        }
        let first_var = match prefix.kind {
            PrefixExpKind::Var(var) => var,
            _ => unreachable!(),
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
        let span = vars.last().map(|v| v.span.merge(end_span)).unwrap_or(end_span);
        Ok(Stat::Assign(StatAssign { span, vars, exprs }))
    }
}