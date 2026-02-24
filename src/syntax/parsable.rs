//! The `Parsable` trait definition.
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
//! - `parse_exp_prec`      – private Pratt recursion helper
//! - `parse_unary`         – private Pratt recursion helper
//! - `parse_primary`       – private Pratt recursion helper
//! - `can_start_lambda` / `skip_*` – speculative lookahead helpers

use crate::syntax::parser::{ParseError, Parser};

/// A node that knows how to parse itself from the token stream.
///
/// The trait is `pub(super)` — it is an implementation detail of the `syntax`
/// module and is not exposed to callers.
pub(super) trait Parsable: Sized {
    fn parse(p: &mut Parser) -> Result<Self, ParseError>;
}