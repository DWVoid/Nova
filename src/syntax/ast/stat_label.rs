use super::name::Name;
use super::stat::Stat;
use crate::lexical::{Keyword, Span, Symbol, TokenKind};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatGoto {
    pub span: Span,
    pub label: Name,
}

impl StatGoto {
    pub fn new(span: Span, label: Name) -> Stat {
        Stat::Goto(StatGoto { span, label })
    }
}

impl Parsable for StatGoto {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Goto)?;
        let label = Name::parse(p)?;
        Ok(StatGoto {
            span: token.span.merge(label.span),
            label,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatLabel {
    pub span: Span,
    pub label: Name,
}

impl StatLabel {
    pub fn new(span: Span, label: Name) -> Stat {
        Stat::Label(StatLabel { span, label })
    }
}

impl Parsable for StatLabel {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.expect_symbol(Symbol::Colon)?;
        p.expect_symbol(Symbol::Colon)?;
        let label = Name::parse(p)?;
        p.expect_symbol(Symbol::Colon)?;
        let end = p.expect_symbol(Symbol::Colon)?;
        Ok(StatLabel {
            span: start.span.merge(end.span),
            label,
        })
    }
}

/// Returns true if the current position looks like the start of a `::label::`.
pub(super) fn is_label_start(p: &Parser) -> bool {
    matches!(p.current().kind, TokenKind::Symbol(Symbol::Colon))
        && matches!(p.peek(1).kind, TokenKind::Symbol(Symbol::Colon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::lex;
    use crate::syntax::parser::Parser;

    fn parser(src: &str) -> Parser {
        let r = lex(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    // ── StatGoto ──────────────────────────────────────────────────────────

    #[test]
    fn parses_goto() {
        let s = StatGoto::parse(&mut parser("goto lbl")).unwrap();
        assert_eq!(s.label.value, "lbl");
    }
    #[test]
    fn goto_rejects_missing_label() {
        assert!(StatGoto::parse(&mut parser("goto")).is_err());
    }
    #[test]
    fn goto_rejects_missing_keyword() {
        assert!(StatGoto::parse(&mut parser("lbl")).is_err());
    }

    // ── StatLabel ─────────────────────────────────────────────────────────

    #[test]
    fn parses_label() {
        let s = StatLabel::parse(&mut parser("::lbl::")).unwrap();
        assert_eq!(s.label.value, "lbl");
    }
    #[test]
    fn label_rejects_single_colon() {
        assert!(StatLabel::parse(&mut parser(":lbl:")).is_err());
    }
    #[test]
    fn label_rejects_missing_name() {
        assert!(StatLabel::parse(&mut parser("::::")).is_err());
    }
}
