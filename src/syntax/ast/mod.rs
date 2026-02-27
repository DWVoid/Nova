//! AST node types for the Nova syntax tree.
//!
//! Each struct/enum lives in its own submodule file; this module re-exports
//! everything so callers continue to use `crate::syntax::ast::*` unchanged.
#![allow(unused)]

mod args;
mod block;
mod chunk;
mod decorator;
mod def_enum;
mod def_struct;
mod def_trait;
mod def_variant;
mod exp;
mod exp_binary;
mod exp_bool;
mod exp_call;
mod exp_field;
mod exp_index;
mod exp_lambda;
mod exp_name;
mod exp_nil;
mod exp_number;
mod exp_paren;
mod exp_string;
mod exp_unary;
mod exp_var_decl;
mod field;
mod initializer;
mod name;
mod ops;
mod param;
mod stat;
mod stat_assign;
mod stat_call;
mod stat_do;
mod stat_empty;
mod stat_for;
mod stat_if;
mod stat_jump;
mod stat_label;
mod stat_repeat;
mod stat_return;
mod stat_while;
mod top;
mod type_spec;
mod use_decl;
mod visibility;

// Re-export every public type so the rest of the crate sees them as
// `crate::syntax::ast::Foo` exactly as before.
pub use args::{Args, ArgsKind};
pub use block::Block;
pub use chunk::{Chunk, NamespaceDecl};
pub use decorator::Decorator;
pub use def_enum::{EnumDef, EnumMember};
pub use def_struct::{FieldDecl, StructDef};
pub use def_trait::{TraitDef, TraitSig};
pub use def_variant::{VariantDef, VariantMember};
pub use exp::Exp;
pub use exp_binary::ExpBinary;
pub use exp_bool::ExpBool;
pub use exp_call::ExpCall;
pub use exp_field::ExpField;
pub use exp_index::ExpIndex;
pub use exp_lambda::ExpLambda;
pub use exp_name::ExpName;
pub use exp_nil::ExpNil;
pub use exp_number::ExpNumber;
pub use exp_paren::ExpParen;
pub use exp_string::ExpString;
pub use exp_unary::ExpUnary;
pub use exp_var_decl::{ExpVarDecl, VarDeclKind};
pub use field::{Field, FieldKey};
pub use initializer::Initializer;
pub use name::Name;
pub use ops::{BinOp, UnOp};
pub use param::Param;
pub use stat::Stat;
pub use stat_assign::StatAssign;
pub use stat_call::StatCall;
pub use stat_do::StatDo;
pub use stat_empty::StatEmpty;
pub use stat_for::{StatForGeneric, StatForNumeric};
pub use stat_if::{IfClause, StatIf};
pub use stat_jump::{StatBreak, StatContinue};
pub use stat_label::{StatGoto, StatLabel};
pub use stat_repeat::StatRepeat;
pub use stat_return::StatReturn;
pub use stat_while::StatWhile;
pub use top::{DefExpr, Definition, Implementation, TopItem};
pub use type_spec::{TypeName, TypeSpec};
pub use use_decl::{UseDecl, UseItem, UseTail};
pub use visibility::Visibility;

/// A list of trivia items (whitespace and comments) attached to a chunk.
pub type Trivia = Vec<crate::lexical::Trivia>;