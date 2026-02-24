//! The `Parsable` trait and its implementations for all AST node types.
//!
//! # Design boundary
//! `Parsable` is implemented for every node type whose parse function takes no
//! arguments beyond the parser itself.  The following helpers intentionally
//! remain as `impl Parser` methods because they are either aggregators that
//! produce `Vec<T>`, purely internal recursive machinery, or tiny shared
//! building-blocks:
//!
//! - `parse_exp_list`      – produces `Vec<Exp>`, not a single AST node
//! - `parse_param_list`    – produces `Vec<Param>`, not a single AST node
//! - `parse_namespace_path`– produces `Vec<Name>`, used by multiple nodes
//! - `parse_decorators`    – produces `Vec<Decorator>`, orchestration helper
//! - `parse_name`          – tiny leaf shared building-block; `Name::parse`
//!                           delegates to it so the two co-exist cleanly
//! - `parse_exp_prec`      – private Pratt recursion helper
//! - `parse_unary`         – private Pratt recursion helper
//! - `parse_primary`       – private Pratt recursion helper
//! - `can_start_lambda` / `skip_*` – speculative lookahead helpers
//! - `parse_prefix_suffixes`– internal suffix-chaining loop for `PrefixExp`
//! - `is_args_start`       – single-token predicate helper

use super::parser::{ParseError, Parser};
use crate::lexical::{Keyword, Span, Symbol, TokenKind};
use crate::syntax::ast::{
    Block, Decorator
    , Name
    , Param, RetStat, Stat
    , TypeName, TypeSpec
    , Visibility,
};

/// A node that knows how to parse itself from the token stream.
///
/// The trait is `pub(super)` — it is an implementation detail of the `syntax`
/// module and is not exposed to callers.
pub(super) trait Parsable: Sized {
    fn parse(p: &mut Parser) -> Result<Self, ParseError>;
}

// ---------------------------------------------------------------------------
// Leaf nodes
// ---------------------------------------------------------------------------

impl Parsable for Name {
    /// Delegates to `Parser::parse_name`, which stays on `Parser` as a shared
    /// building-block called by collection helpers (`parse_namespace_path`, etc.).
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.current().clone();
        if let TokenKind::Identifier(value) = token.kind {
            p.advance();
            return Ok(Name {
                value,
                span: token.span,
            });
        }
        Err(ParseError {
            message: "expected identifier".to_string(),
            position: token.span.start,
        })
    }
}


// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

impl Parsable for TypeSpec {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let colon = p.expect_symbol(Symbol::Colon)?;
        let ty = TypeName::parse(p)?;
        let span = colon.span.merge(ty.span);
        Ok(TypeSpec { span, ty })
    }
}

impl Parsable for TypeName {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let parts = p.parse_namespace_path()?;
        let mut span = parts[0].span;
        for name in &parts[1..] {
            span = span.merge(name.span);
        }
        Ok(TypeName { span, parts })
    }
}

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

impl Parsable for Visibility {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Export)?;
        let scopes = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let mut names = Vec::new();
            if !p.is_symbol(Symbol::RParen) {
                names.push(Name::parse(p)?);
                while p.is_symbol(Symbol::Comma) {
                    p.advance();
                    names.push(Name::parse(p)?);
                }
            }
            p.expect_symbol(Symbol::RParen)?;
            Some(names)
        } else {
            None
        };
        Ok(Visibility {
            span: token.span,
            scopes,
        })
    }
}

// ---------------------------------------------------------------------------
// Decorator
// ---------------------------------------------------------------------------

impl Parsable for Decorator {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let at = p.expect_symbol(Symbol::At)?;
        let name = Name::parse(p)?;
        let mut span = at.span.merge(name.span);
        let args = if p.is_symbol(Symbol::LParen) {
            p.advance();
            let args = if p.is_symbol(Symbol::RParen) {
                Vec::new()
            } else {
                p.parse_exp_list()?
            };
            let close = p.expect_symbol(Symbol::RParen)?;
            span = span.merge(close.span);
            Some(args)
        } else {
            None
        };
        Ok(Decorator { span, name, args })
    }
}

impl Parsable for Param {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let name = Name::parse(p)?;
        let type_spec = if p.is_symbol(Symbol::Colon) {
            Some(TypeSpec::parse(p)?)
        } else {
            None
        };
        Ok(Param { name, type_spec })
    }
}

impl Parsable for Block {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let start = p.current().span.start;
        let mut end_pos = start;
        let mut stats = Vec::new();
        let mut ret = None;

        while !p.is_block_end() {
            if p.is_keyword(Keyword::Return) {
                let retstat = RetStat::parse(p)?;
                end_pos = retstat.span.end;
                ret = Some(retstat);
                if p.is_symbol(Symbol::Semi) {
                    let semi = p.advance();
                    end_pos = semi.span.end;
                }
                break;
            }
            let stat = Stat::parse(p)?;
            end_pos = stat.span.end;
            stats.push(stat);
        }

        let span = Span::new(start, end_pos);
        Ok(Block { span, stats, ret })
    }
}

impl Parsable for RetStat {
    fn parse(p: &mut Parser) -> Result<Self, ParseError> {
        let token = p.expect_keyword(Keyword::Return)?;
        let mut exprs = Vec::new();
        if !p.is_block_end() && !p.is_symbol(Symbol::Semi) {
            exprs = p.parse_exp_list()?;
        }
        let end_span = exprs.last().map(|e| e.span).unwrap_or(token.span);
        Ok(RetStat {
            span: token.span.merge(end_span),
            exprs,
        })
    }
}

