use super::args::Args;
use super::exp_binary::ExpBinary;
use super::exp_bool::ExpBool;
use super::exp_call::ExpCall;
use super::exp_field::ExpField;
use super::exp_index::ExpIndex;
use super::exp_lambda::ExpLambda;
use super::exp_name::ExpName;
use super::exp_nil::ExpNil;
use super::exp_number::ExpNumber;
use super::exp_paren::ExpParen;
use super::exp_string::ExpString;
use super::exp_unary::ExpUnary;
use super::exp_var_decl::{ExpVarDecl, VarDeclKind};
use super::name::Name;
use super::type_spec::TypeSpec;
use crate::lexical::Span;
use crate::lexical::{Keyword, Symbol, TokenKind};
use crate::syntax::ast::{BinOp, UnOp};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::Serialize;
use crate::syntax::syntax::SyntaxError;

/// Operator associativity, used by the Pratt expression parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Assoc {
    Left,
    Right,
}

/// A fully unified expression node.
///
/// Each variant is a distinct struct that carries the node's span and fields.
/// Whether a node is a valid l-value, callable expression, or pure value is
/// not checked by the parser — that distinction is deferred to the semantic stage.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Exp {
    // ── Literals ──────────────────────────────────────────────────────────
    Nil(ExpNil),
    Bool(ExpBool),
    Number(ExpNumber),
    String(ExpString),

    // ── Atomic / postfix forms ────────────────────────────────────────────
    /// A bare identifier: `foo`.
    Name(ExpName),
    /// A parenthesised sub-expression: `( exp )`.
    Paren(ExpParen),
    /// Field access: `prefix.name`.
    Field(ExpField),
    /// Index access: `prefix[index]`.
    Index(ExpIndex),
    /// A function call: `prefix(args)`.
    Call(ExpCall),
    /// A local binding site: `var name [: T]` or `val name [: T]`.
    VarDecl(ExpVarDecl),

    // ── Operator forms ────────────────────────────────────────────────────
    Lambda(ExpLambda),
    Unary(ExpUnary),
    Binary(ExpBinary),
}

impl Exp {
    /// Return the source span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Exp::Nil(e) => e.span,
            Exp::Bool(e) => e.span,
            Exp::Number(e) => e.span,
            Exp::String(e) => e.span,
            Exp::Name(e) => e.span,
            Exp::Paren(e) => e.span,
            Exp::Field(e) => e.span,
            Exp::Index(e) => e.span,
            Exp::Call(e) => e.span,
            Exp::VarDecl(e) => e.span,
            Exp::Lambda(e) => e.span,
            Exp::Unary(e) => e.span,
            Exp::Binary(e) => e.span,
        }
    }
}

// ── Parsable ──────────────────────────────────────────────────────────────────

impl Parsable for Exp {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        Self::parse_prec(p, 0)
    }
}

/// Parse a non-empty comma-separated expression list.
impl Parsable for Vec<Exp> {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let mut exprs = vec![Exp::parse(p)?];
        while p.is_symbol(Symbol::Comma) {
            p.advance();
            exprs.push(Exp::parse(p)?);
        }
        Ok(exprs)
    }
}

impl Exp {
    // ── Pratt driver ─────────────────────────────────────────────────────

    fn parse_prec(p: &mut Parser, min_prec: u8) -> Result<Exp, SyntaxError> {
        let mut left = Self::parse_unary(p)?;
        loop {
            let Some((op, prec, assoc)) = Self::peek_binop(p) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            p.advance();
            let next_min = if assoc == Assoc::Left { prec + 1 } else { prec };
            let right = Self::parse_prec(p, next_min)?;
            let span = left.span().merge(right.span());
            left = ExpBinary::new(span, op, left, right);
        }
        Ok(left)
    }

    fn parse_unary(p: &mut Parser) -> Result<Exp, SyntaxError> {
        if let Some(op) = Self::peek_unop(p) {
            let token = p.advance();
            let exp = Self::parse_unary(p)?;
            let span = token.span.merge(exp.span());
            return Ok(ExpUnary::new(span, op, exp));
        }
        Self::parse_postfix(p)
    }

    // ── Primary + postfix suffix chaining ────────────────────────────────

    fn parse_postfix(p: &mut Parser) -> Result<Exp, SyntaxError> {
        let mut base = Self::parse_primary(p)?;
        loop {
            if p.is_symbol(Symbol::Dot) {
                p.advance();
                let name = Name::parse(p)?;
                let span = base.span().merge(name.span);
                base = ExpField::new(span, base, name);
                continue;
            }
            if p.is_symbol(Symbol::LBracket) {
                p.advance();
                let index = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RBracket)?;
                let span = base.span().merge(close.span);
                base = ExpIndex::new(span, base, index);
                continue;
            }
            if matches!(
                p.current().kind,
                TokenKind::Symbol(Symbol::LParen) | TokenKind::Symbol(Symbol::LBrace)
            ) {
                let args = Args::parse(p)?;
                let span = base.span().merge(args.span);
                base = ExpCall::new(span, base, args);
                continue;
            }
            break;
        }
        Ok(base)
    }

    fn parse_primary(p: &mut Parser) -> Result<Exp, SyntaxError> {
        let token = p.current().clone();
        match token.kind {
            TokenKind::Number(text) => {
                p.advance();
                Ok(ExpNumber::new(token.span, text))
            }
            TokenKind::StringLiteral(text) => {
                p.advance();
                Ok(ExpString::new(token.span, text))
            }
            TokenKind::Keyword(Keyword::Nil) => {
                p.advance();
                Ok(ExpNil::new(token.span))
            }
            TokenKind::Keyword(Keyword::True) => {
                p.advance();
                Ok(ExpBool::new(token.span, true))
            }
            TokenKind::Keyword(Keyword::False) => {
                p.advance();
                Ok(ExpBool::new(token.span, false))
            }
            TokenKind::Identifier(name) => {
                p.advance();
                Ok(ExpName::new(token.span, name))
            }
            TokenKind::Keyword(Keyword::Var) | TokenKind::Keyword(Keyword::Val) => {
                let decl_kind = if matches!(token.kind, TokenKind::Keyword(Keyword::Var)) {
                    VarDeclKind::Var
                } else {
                    VarDeclKind::Val
                };
                p.advance();
                let name = Name::parse(p)?;
                let type_spec = if p.is_symbol(Symbol::Colon) {
                    Some(TypeSpec::parse(p)?)
                } else {
                    None
                };
                let mut span = token.span.merge(name.span);
                if let Some(ts) = &type_spec {
                    span = span.merge(ts.span);
                }
                Ok(ExpVarDecl::new(span, decl_kind, name, type_spec))
            }
            TokenKind::Symbol(Symbol::LParen) => {
                if Self::can_start_lambda(p)? {
                    let lambda = ExpLambda::parse(p)?;
                    return Ok(ExpLambda::new_exp(lambda));
                }
                let open = p.advance();
                let inner = Exp::parse(p)?;
                let close = p.expect_symbol(Symbol::RParen)?;
                Ok(ExpParen::new(open.span.merge(close.span), inner))
            }
            _ => Err(SyntaxError {
                message: format!("unexpected token in expression: {:?}", token.kind),
                position: token.span.start,
            }),
        }
    }

    // ── Lambda lookahead ──────────────────────────────────────────────────
    //
    // A `(` starts a lambda expression if and only if the token immediately
    // after its matching `)` is `:` (the return-type annotation).
    //
    // A plain parenthesised expression `(expr)` can never be followed by `:`
    // in a position where an expression is expected, because `:` is not a
    // binary operator.  `const` now appears *after* the return type, so it
    // plays no role in distinguishing the two forms at the leading `(`.
    //
    // The scan only needs to track paren nesting depth — it never needs to
    // understand the token content.
    fn can_start_lambda(p: &mut Parser) -> Result<bool, SyntaxError> {
        let checkpoint = p.checkpoint();
        // Must start with `(`
        if !p.is_symbol(Symbol::LParen) {
            p.restore(checkpoint);
            return Ok(false);
        }
        // Walk forward, tracking nesting, until the matching `)` is found.
        let mut depth: usize = 0;
        loop {
            match p.current().kind {
                TokenKind::Symbol(Symbol::LParen) => {
                    depth += 1;
                    p.advance();
                }
                TokenKind::Symbol(Symbol::RParen) => {
                    depth -= 1;
                    p.advance();
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Eof => {
                    p.restore(checkpoint);
                    return Ok(false);
                }
                _ => {
                    p.advance();
                }
            }
        }
        // A `:` here means this is a lambda parameter list followed by a return type.
        let result = p.is_symbol(Symbol::Colon);
        p.restore(checkpoint);
        Ok(result)
    }

    fn peek_unop(p: &mut Parser) -> Option<UnOp> {
        match p.current().kind {
            TokenKind::Keyword(Keyword::Not) => Some(UnOp::Not),
            TokenKind::Symbol(Symbol::Minus) => Some(UnOp::Neg),
            TokenKind::Symbol(Symbol::Hash) => Some(UnOp::Len),
            TokenKind::Symbol(Symbol::Tilde) => Some(UnOp::BitNot),
            _ => None,
        }
    }
    fn peek_binop(p: &mut Parser) -> Option<(BinOp, u8, Assoc)> {
        match p.current().kind {
            TokenKind::Keyword(Keyword::Or) => Some((BinOp::Or, 1, Assoc::Left)),
            TokenKind::Keyword(Keyword::And) => Some((BinOp::And, 2, Assoc::Left)),
            TokenKind::Symbol(Symbol::Less)
            | TokenKind::Symbol(Symbol::LessEq)
            | TokenKind::Symbol(Symbol::Greater)
            | TokenKind::Symbol(Symbol::GreaterEq)
            | TokenKind::Symbol(Symbol::EqEq)
            | TokenKind::Symbol(Symbol::NotEq) => {
                Some((Self::binop_from_symbol(p)?, 3, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::Pipe) => Some((BinOp::BitOr, 4, Assoc::Left)),
            TokenKind::Symbol(Symbol::Tilde) => Some((BinOp::BitXor, 5, Assoc::Left)),
            TokenKind::Symbol(Symbol::Amp) => Some((BinOp::BitAnd, 6, Assoc::Left)),
            TokenKind::Symbol(Symbol::ShiftLeft) | TokenKind::Symbol(Symbol::ShiftRight) => {
                Some((Self::binop_from_symbol(p)?, 7, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::DotDot) => Some((BinOp::Concat, 8, Assoc::Right)),
            TokenKind::Symbol(Symbol::Plus) | TokenKind::Symbol(Symbol::Minus) => {
                Some((Self::binop_from_symbol(p)?, 9, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::Star)
            | TokenKind::Symbol(Symbol::Slash)
            | TokenKind::Symbol(Symbol::FloorDiv)
            | TokenKind::Symbol(Symbol::Percent) => {
                Some((Self::binop_from_symbol(p)?, 10, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::Caret) => Some((BinOp::Pow, 12, Assoc::Right)),
            _ => None,
        }
    }
    fn binop_from_symbol(p: &mut Parser) -> Option<BinOp> {
        match p.current().kind {
            TokenKind::Symbol(Symbol::Less) => Some(BinOp::Less),
            TokenKind::Symbol(Symbol::LessEq) => Some(BinOp::LessEq),
            TokenKind::Symbol(Symbol::Greater) => Some(BinOp::Greater),
            TokenKind::Symbol(Symbol::GreaterEq) => Some(BinOp::GreaterEq),
            TokenKind::Symbol(Symbol::EqEq) => Some(BinOp::Eq),
            TokenKind::Symbol(Symbol::NotEq) => Some(BinOp::NotEq),
            TokenKind::Symbol(Symbol::ShiftLeft) => Some(BinOp::ShiftLeft),
            TokenKind::Symbol(Symbol::ShiftRight) => Some(BinOp::ShiftRight),
            TokenKind::Symbol(Symbol::Plus) => Some(BinOp::Add),
            TokenKind::Symbol(Symbol::Minus) => Some(BinOp::Sub),
            TokenKind::Symbol(Symbol::Star) => Some(BinOp::Mul),
            TokenKind::Symbol(Symbol::Slash) => Some(BinOp::Div),
            TokenKind::Symbol(Symbol::FloorDiv) => Some(BinOp::FloorDiv),
            TokenKind::Symbol(Symbol::Percent) => Some(BinOp::Mod),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ops::{BinOp, UnOp};
    use super::*;
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    fn parse(src: &str) -> Exp {
        Exp::parse(&mut parser(src)).unwrap()
    }

    fn parse_err(src: &str) -> bool {
        Exp::parse(&mut parser(src)).is_err()
    }

    // ── Literals ──────────────────────────────────────────────────────────

    #[test]
    fn parses_nil() {
        assert!(matches!(parse("nil"), Exp::Nil(_)));
    }
    #[test]
    fn parses_true() {
        assert!(matches!(
            parse("true"),
            Exp::Bool(ExpBool { value: true, .. })
        ));
    }
    #[test]
    fn parses_false() {
        assert!(matches!(
            parse("false"),
            Exp::Bool(ExpBool { value: false, .. })
        ));
    }
    #[test]
    fn parses_number() {
        assert!(matches!(parse("42"), Exp::Number(_)));
    }
    #[test]
    fn parses_string() {
        assert!(matches!(parse("\"hi\""), Exp::String(_)));
    }

    // ── Atomic forms ──────────────────────────────────────────────────────

    #[test]
    fn parses_name() {
        assert!(matches!(parse("foo"), Exp::Name(ExpName { ref name, .. }) if name == "foo"));
    }

    #[test]
    fn parses_paren() {
        assert!(matches!(parse("(1)"), Exp::Paren(_)));
    }

    // ── VarDecl ───────────────────────────────────────────────────────────

    #[test]
    fn parses_var_decl_no_type() {
        assert!(matches!(
            parse("var x"),
            Exp::VarDecl(ExpVarDecl {
                kind: VarDeclKind::Var,
                ..
            })
        ));
    }

    #[test]
    fn parses_val_decl_with_type() {
        assert!(matches!(
            parse("val x: Foo"),
            Exp::VarDecl(ExpVarDecl {
                kind: VarDeclKind::Val,
                ..
            })
        ));
    }

    // ── Postfix: Field, Index, Call ───────────────────────────────────────

    #[test]
    fn parses_field_access() {
        assert!(matches!(parse("a.b"), Exp::Field(_)));
    }

    #[test]
    fn parses_chained_field() {
        assert!(matches!(parse("a.b.c"), Exp::Field(_)));
    }

    #[test]
    fn parses_index_access() {
        assert!(matches!(parse("a[0]"), Exp::Index(_)));
    }

    #[test]
    fn parses_call_no_args() {
        assert!(matches!(parse("f()"), Exp::Call(_)));
    }

    #[test]
    fn parses_call_with_args() {
        assert!(matches!(parse("f(1, 2)"), Exp::Call(_)));
    }

    #[test]
    fn parses_call_initializer_arg() {
        assert!(matches!(parse("f{1}"), Exp::Call(_)));
    }

    #[test]
    fn parses_chained_call() {
        assert!(matches!(parse("f()()"), Exp::Call(_)));
    }

    // ── Unary ─────────────────────────────────────────────────────────────

    #[test]
    fn parses_unary_neg() {
        assert!(matches!(
            parse("-1"),
            Exp::Unary(ExpUnary { op: UnOp::Neg, .. })
        ));
    }
    #[test]
    fn parses_unary_not() {
        assert!(matches!(
            parse("not x"),
            Exp::Unary(ExpUnary { op: UnOp::Not, .. })
        ));
    }
    #[test]
    fn parses_unary_len() {
        assert!(matches!(
            parse("#x"),
            Exp::Unary(ExpUnary { op: UnOp::Len, .. })
        ));
    }
    #[test]
    fn parses_unary_bitnot() {
        assert!(matches!(
            parse("~x"),
            Exp::Unary(ExpUnary {
                op: UnOp::BitNot,
                ..
            })
        ));
    }

    // ── Binary ────────────────────────────────────────────────────────────

    #[test]
    fn parses_binary_add() {
        assert!(matches!(parse("1 + 2"), Exp::Binary(ref b) if b.op == BinOp::Add));
    }

    #[test]
    fn parses_binary_and() {
        assert!(matches!(parse("a and b"), Exp::Binary(ref b) if b.op == BinOp::And));
    }

    #[test]
    fn precedence_mul_over_add() {
        let e = parse("1 + 2 * 3");
        if let Exp::Binary(ref b) = e {
            assert_eq!(b.op, BinOp::Add);
            assert!(matches!(*b.right, Exp::Binary(ref r) if r.op == BinOp::Mul));
        } else {
            panic!("expected add at root");
        }
    }

    #[test]
    fn right_assoc_pow() {
        let e = parse("2 ^ 3 ^ 4");
        if let Exp::Binary(ref b) = e {
            assert_eq!(b.op, BinOp::Pow);
            assert!(matches!(*b.right, Exp::Binary(ref r) if r.op == BinOp::Pow));
        } else {
            panic!("expected pow at root");
        }
    }

    // ── Lambda ────────────────────────────────────────────────────────────

    #[test]
    fn parses_lambda_expr() {
        assert!(matches!(parse("(): unit end"), Exp::Lambda(_)));
    }

    #[test]
    fn parses_const_lambda_expr() {
        assert!(matches!(parse("(): unit const end"), Exp::Lambda(_)));
    }

    // ── Rejections ────────────────────────────────────────────────────────

    #[test]
    fn rejects_bare_keyword_as_exp() {
        assert!(parse_err("end"));
    }

    #[test]
    fn rejects_const_without_lambda() {
        assert!(parse_err("const 42"));
    }
}
