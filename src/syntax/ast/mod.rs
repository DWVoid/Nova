//! AST node types for the Nova syntax tree.
//!
//! Each struct/enum lives in its own submodule file; this module re-exports
//! everything so callers continue to use `crate::syntax::ast::*` unchanged.

mod args;
mod block;
mod defs;
mod exp;
mod field;
mod function_call;
mod if_clause;
mod initializer;
mod lambda_expr;
mod name;
mod ops;
mod param;
mod prefix_exp;
mod ret_stat;
mod stat;
mod top;
mod type_spec;
mod var;

// Re-export every public type so the rest of the crate sees them as
// `crate::syntax::ast::Foo` exactly as before.
pub use args::{Args, ArgsKind};
pub use block::Block;
pub use defs::{
    EnumDef, EnumMember, FieldDecl, StructDef, TraitDef, TraitSig, VariantDef, VariantMember,
};
pub use exp::{Exp, ExpKind};
pub use field::{Field, FieldKey};
pub use function_call::FunctionCall;
pub use if_clause::IfClause;
pub use initializer::Initializer;
pub use lambda_expr::LambdaExpr;
pub use name::Name;
pub use ops::{BinOp, UnOp};
pub use param::Param;
pub use prefix_exp::{PrefixExp, PrefixExpKind};
pub use ret_stat::RetStat;
pub use stat::{Stat, StatKind};
pub use top::{
    Chunk, Decorator, DefExpr, Definition, Implementation, NamespaceDecl, TopItem, UseDecl,
    UseItem, UseTail, Visibility,
};
pub use type_spec::{TypeName, TypeSpec};
pub use var::{Var, VarDeclKind, VarKind};

/// A list of trivia items (whitespace and comments) attached to a chunk.
pub type Trivia = Vec<crate::lexical::Trivia>;
