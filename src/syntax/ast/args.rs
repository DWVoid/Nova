use super::exp::Exp;
use super::initializer::Initializer;
use crate::lexical::Span;
use crate::lexical::{Symbol, TokenKind};
use crate::syntax::parsable::Parsable;
use crate::syntax::parser::{ParseError, Parser};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ArgsKind {
    ExpList(Vec<Exp>),
    Initializer(Initializer),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Args {
    pub span: Span,
    pub kind: ArgsKind,
}

impl Parsable for Args {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Symbol(Symbol::LParen) => {
                let open = p.advance();
                let mut exprs = Vec::new();
                if !p.is_symbol(Symbol::RParen) {
                    exprs = Vec::<Exp>::parse(p)?;
                }
                let close = p.expect_symbol(Symbol::RParen)?;
                Ok(Args {
                    span: open.span.merge(close.span),
                    kind: ArgsKind::ExpList(exprs),
                })
            }
            TokenKind::Symbol(Symbol::LBrace) => {
                let init = Initializer::parse(p)?;
                Ok(Args {
                    span: init.span,
                    kind: ArgsKind::Initializer(init),
                })
            }
            _ => Err(ParseError {
                message: "expected call arguments".to_string(),
                position: token.span.start,
            }),
        }
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
    fn parses_empty_arg_list() {
        let mut p = parser("()");
        let a = Args::parse(&mut p).unwrap();
        assert!(matches!(a.kind, ArgsKind::ExpList(ref v) if v.is_empty()));
    }

    #[test]
    fn parses_arg_list_with_exprs() {
        let mut p = parser("(1, 2, 3)");
        let a = Args::parse(&mut p).unwrap();
        assert!(matches!(a.kind, ArgsKind::ExpList(ref v) if v.len() == 3));
    }

    #[test]
    fn parses_initializer_args() {
        let mut p = parser("{1, 2}");
        let a = Args::parse(&mut p).unwrap();
        assert!(matches!(a.kind, ArgsKind::Initializer(_)));
    }

    #[test]
    fn rejects_non_args() {
        let mut p = parser("foo");
        assert!(Args::parse(&mut p).is_err());
    }
}
