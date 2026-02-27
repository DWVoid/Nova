use super::exp::Exp;
use super::stat::Stat;
use crate::lexical::Keyword;
use crate::lexical::Span;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatReturn {
    pub span: Span,
    pub exprs: Vec<Exp>,
}

impl StatReturn {
    pub fn new(span: Span, exprs: Vec<Exp>) -> Stat {
        Stat::Return(StatReturn { span, exprs })
    }
}

impl Parsable for StatReturn {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !p.is_block_end() && !p.is_symbol(crate::lexical::Symbol::Semi) {
            exprs = Vec::<Exp>::parse(p)?;
        }
        let end_span = exprs.last().map(|e| e.span()).unwrap_or(token.span);
        Ok(StatReturn {
            span: token.span.merge(end_span),
            exprs,
        })
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

    #[test]
    fn parses_return_no_value() {
        let mut p = parser("return end");
        let r = StatReturn::parse(&mut p).unwrap();
        assert!(r.exprs.is_empty());
    }

    #[test]
    fn parses_return_with_value() {
        let mut p = parser("return 1 end");
        let r = StatReturn::parse(&mut p).unwrap();
        assert_eq!(r.exprs.len(), 1);
    }

    #[test]
    fn parses_return_multiple_values() {
        let mut p = parser("return 1, 2 end");
        let r = StatReturn::parse(&mut p).unwrap();
        assert_eq!(r.exprs.len(), 2);
    }

    #[test]
    fn rejects_missing_return_keyword() {
        let mut p = parser("1");
        assert!(StatReturn::parse(&mut p).is_err());
    }
}
