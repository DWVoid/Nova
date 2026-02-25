use super::exp::Exp;
use super::lambda_expr::LambdaExpr;
use crate::lexical::Span;
use serde::Serialize;

/// A lambda expression used as a value: `(params): RetType block end`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExpLambda {
    pub span: Span,
    pub lambda: LambdaExpr,
}

impl ExpLambda {
    pub fn new(lambda: LambdaExpr) -> Exp {
        let span = lambda.span;
        Exp::Lambda(ExpLambda { span, lambda })
    }
}
