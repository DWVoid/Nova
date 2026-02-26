use super::block::Block;
use super::param::Param;
use super::type_spec::TypeSpec;
use crate::lexical::Keyword;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LambdaExpr {
    pub span: Span,
    pub is_const: bool,
    pub params: Vec<Param>,
    pub return_type: TypeSpec,
    pub block: Block,
}

impl Parsable for LambdaExpr {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.current().span.start;
        let params = p.parse_param_list()?;
        let return_type = TypeSpec::parse(p)?;
        let is_const = if p.is_keyword(Keyword::Const) {
            p.advance();
            true
        } else {
            false
        };
        let block = Block::parse(p)?;
        let end = p.expect_keyword(Keyword::End)?;
        let span = Span::new(start, end.span.end);
        Ok(LambdaExpr {
            span,
            is_const,
            params,
            return_type,
            block,
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
    fn parses_lambda_no_params() {
        let mut p = parser("(): unit end");
        let l = LambdaExpr::parse(&mut p).unwrap();
        assert!(!l.is_const);
        assert!(l.params.is_empty());
        assert_eq!(l.return_type.ty.parts[0].value, "unit");
    }

    #[test]
    fn parses_const_lambda() {
        let mut p = parser("(): unit const end");
        let l = LambdaExpr::parse(&mut p).unwrap();
        assert!(l.is_const);
    }

    #[test]
    fn parses_lambda_with_params() {
        let mut p = parser("(x: int, y: int): int end");
        let l = LambdaExpr::parse(&mut p).unwrap();
        assert_eq!(l.params.len(), 2);
    }

    #[test]
    fn rejects_lambda_missing_return_type() {
        let mut p = parser("() end");
        assert!(LambdaExpr::parse(&mut p).is_err());
    }

    #[test]
    fn rejects_lambda_missing_end() {
        let mut p = parser("(): unit");
        assert!(LambdaExpr::parse(&mut p).is_err());
    }
}
