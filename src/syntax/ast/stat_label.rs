use serde::Serialize;
use crate::lexical::{Keyword, Span, Symbol, TokenKind};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use super::name::Name;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatGoto {
    pub span: Span,
    pub label: Name,
}

impl Parsable for StatGoto {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Goto)?;
        let label = Name::parse(p)?;
        Ok(StatGoto { span: token.span.merge(label.span), label })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatLabel {
    pub span: Span,
    pub label: Name,
}

impl Parsable for StatLabel {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.expect_symbol(Symbol::Colon)?;
        p.expect_symbol(Symbol::Colon)?;
        let label = Name::parse(p)?;
        p.expect_symbol(Symbol::Colon)?;
        let end = p.expect_symbol(Symbol::Colon)?;
        Ok(StatLabel { span: start.span.merge(end.span), label })
    }
}

/// Returns true if the current position looks like the start of a `::label::`.
pub(super) fn is_label_start(p: &Parser) -> bool {
    matches!(p.current().kind, TokenKind::Symbol(Symbol::Colon))
        && matches!(p.peek(1).kind, TokenKind::Symbol(Symbol::Colon))
}
