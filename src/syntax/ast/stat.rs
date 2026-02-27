use super::exp::Exp;
use super::stat_assign::StatAssign;
use super::stat_call::StatCall;
use super::stat_do::StatDo;
use super::stat_empty::StatEmpty;
use super::stat_for::{StatForGeneric, StatForNumeric};
use super::stat_if::StatIf;
use super::stat_jump::{StatBreak, StatContinue};
use super::stat_label::{is_label_start, StatGoto, StatLabel};
use super::stat_repeat::StatRepeat;
use super::stat_return::StatReturn;
use super::stat_while::StatWhile;
use crate::lexical::{Keyword, Symbol, TokenKind};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Stat {
    Empty(StatEmpty),
    Return(StatReturn),
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
            Stat::Return(s) => s.span,
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
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        if p.is_symbol(Symbol::Semi) {
            return Ok(Stat::Empty(StatEmpty::parse(p)?));
        }
        if p.is_keyword(Keyword::Return) {
            return Ok(Stat::Return(StatReturn::parse(p)?));
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
            // `for name =` → numeric;  `for name[, name…] in` → generic
            return Ok(
                if matches!(p.peek(2).kind, TokenKind::Symbol(Symbol::Assign)) {
                    Stat::ForNumeric(StatForNumeric::parse(p)?)
                } else {
                    Stat::ForGeneric(StatForGeneric::parse(p)?)
                },
            );
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

        // Parse one full expression (including all postfix suffixes).
        let exp = Exp::parse(p)?;

        // A bare parenthesised expression is not a valid statement.
        if matches!(exp, Exp::Paren(_)) {
            return Err(SyntaxError {
                message: "parenthesized expression cannot start a statement; \
                          expected assignment or call"
                    .to_string(),
                position: exp.span().start,
            });
        }

        // If `=` or `,` follows, this is an assignment statement.
        if p.is_symbol(Symbol::Assign) || p.is_symbol(Symbol::Comma) {
            let mut vars = vec![exp];
            while p.is_symbol(Symbol::Comma) {
                p.advance();
                vars.push(Exp::parse(p)?);
            }
            let eq = p.expect_symbol(Symbol::Assign)?;
            let exprs = Vec::<Exp>::parse(p)?;
            let end_span = exprs.last().map(|e| e.span()).unwrap_or(eq.span);
            let span = vars
                .last()
                .map(|v| v.span().merge(end_span))
                .unwrap_or(end_span);
            return Ok(Stat::Assign(StatAssign { span, vars, exprs }));
        }

        // Otherwise the expression must be a call.
        if matches!(exp, Exp::Call(_)) {
            let span = exp.span();
            Ok(Stat::Call(StatCall { span, call: exp }))
        } else {
            Err(SyntaxError {
                message: "expression statement must be a function call or assignment".to_string(),
                position: exp.span().start,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }
    fn stat(src: &str) -> Stat {
        Stat::parse(&mut parser(src)).unwrap()
    }

    #[test]
    fn dispatches_return() {
        assert!(matches!(stat("return end"), Stat::Return(_)));
    }
    #[test]
    fn dispatches_return_value() {
        assert!(matches!(stat("return 1 end"), Stat::Return(_)));
    }
    #[test]
    fn dispatches_empty() {
        assert!(matches!(stat(";"), Stat::Empty(_)));
    }
    #[test]
    fn dispatches_do() {
        assert!(matches!(stat("do end"), Stat::Do(_)));
    }
    #[test]
    fn dispatches_while() {
        assert!(matches!(stat("while x do end"), Stat::While(_)));
    }
    #[test]
    fn dispatches_repeat() {
        assert!(matches!(stat("repeat until x"), Stat::Repeat(_)));
    }
    #[test]
    fn dispatches_if() {
        assert!(matches!(stat("if x then end"), Stat::If(_)));
    }
    #[test]
    fn dispatches_for_numeric() {
        assert!(matches!(stat("for i = 1, 2 do end"), Stat::ForNumeric(_)));
    }
    #[test]
    fn dispatches_for_generic() {
        assert!(matches!(stat("for x in y do end"), Stat::ForGeneric(_)));
    }
    #[test]
    fn dispatches_break() {
        assert!(matches!(stat("break"), Stat::Break(_)));
    }
    #[test]
    fn dispatches_continue() {
        assert!(matches!(stat("continue"), Stat::Continue(_)));
    }
    #[test]
    fn dispatches_goto() {
        assert!(matches!(stat("goto lbl"), Stat::Goto(_)));
    }
    #[test]
    fn dispatches_label() {
        assert!(matches!(stat("::lbl::"), Stat::Label(_)));
    }
    #[test]
    fn dispatches_call() {
        assert!(matches!(stat("f()"), Stat::Call(_)));
    }
    #[test]
    fn dispatches_assign() {
        assert!(matches!(stat("x = 1"), Stat::Assign(_)));
    }

    #[test]
    fn rejects_paren_statement() {
        assert!(Stat::parse(&mut parser("(x)")).is_err());
    }
    #[test]
    fn rejects_bare_name_statement() {
        assert!(Stat::parse(&mut parser("x")).is_err());
    }
}
