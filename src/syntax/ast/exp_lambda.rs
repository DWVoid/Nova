use super::block::Block;
use super::exp::Exp;
use super::param::Param;
use super::type_spec::TypeSpec;
use crate::lexical::Keyword;
use crate::lexical::Span;
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

/// A lambda expression used as a value: `(params): RetType [const] block end`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpLambda {
    pub span: Span,
    pub is_const: bool,
    pub params: Vec<Param>,
    pub return_type: TypeSpec,
    pub block: Block,
}

impl ExpLambda {
    pub fn new_exp(lambda: ExpLambda) -> Exp {
        Exp::Lambda(lambda)
    }
}

impl Parsable for ExpLambda {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
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
        Ok(ExpLambda {
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
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    #[test]
    fn parses_lambda_no_params() {
        let mut p = parser("(): unit end");
        let l = ExpLambda::parse(&mut p).unwrap();
        assert!(!l.is_const);
        assert!(l.params.is_empty());
        assert_eq!(l.return_type.ty.parts[0].value, "unit");
    }

    #[test]
    fn parses_const_lambda() {
        let mut p = parser("(): unit const end");
        let l = ExpLambda::parse(&mut p).unwrap();
        assert!(l.is_const);
    }

    #[test]
    fn parses_lambda_with_params() {
        let mut p = parser("(x: int, y: int): int end");
        let l = ExpLambda::parse(&mut p).unwrap();
        assert_eq!(l.params.len(), 2);
    }

    #[test]
    fn rejects_lambda_missing_return_type() {
        let mut p = parser("() end");
        assert!(ExpLambda::parse(&mut p).is_err());
    }

    #[test]
    fn rejects_lambda_missing_end() {
        let mut p = parser("(): unit");
        assert!(ExpLambda::parse(&mut p).is_err());
    }
}
