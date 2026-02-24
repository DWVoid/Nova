use serde::Serialize;
use crate::lexical::Span;
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use crate::lexical::Keyword;
use super::block::Block;
use super::param::Param;
use super::type_spec::TypeSpec;

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
        let is_const = if p.is_keyword(Keyword::Const) {
            p.advance();
            true
        } else {
            false
        };
        let params = p.parse_param_list()?;
        let return_type = TypeSpec::parse(p)?;
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
