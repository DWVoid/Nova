use super::exp::Exp;
use crate::lexical::Keyword;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RetStat {
    pub span: Span,
    pub exprs: Vec<Exp>,
}

impl Parsable for RetStat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !p.is_block_end() && !p.is_symbol(crate::lexical::Symbol::Semi) {
            exprs = p.parse_exp_list()?;
        }
        let end_span = exprs.last().map(|e| e.span).unwrap_or(token.span);
        Ok(RetStat {
            span: token.span.merge(end_span),
            exprs,
        })
    }
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

    #[test]
    fn parses_return_no_value() {
        let mut p = parser("return end");
        let r = RetStat::parse(&mut p).unwrap();
        assert!(r.exprs.is_empty());
    }

    #[test]
    fn parses_return_with_value() {
        let mut p = parser("return 1 end");
        let r = RetStat::parse(&mut p).unwrap();
        assert_eq!(r.exprs.len(), 1);
    }

    #[test]
    fn parses_return_multiple_values() {
        let mut p = parser("return 1, 2 end");
        let r = RetStat::parse(&mut p).unwrap();
        assert_eq!(r.exprs.len(), 2);
    }

    #[test]
    fn rejects_missing_return_keyword() {
        let mut p = parser("1");
        assert!(RetStat::parse(&mut p).is_err());
    }
}
