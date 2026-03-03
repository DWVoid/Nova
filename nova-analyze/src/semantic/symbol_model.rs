//! # Bundle Symbol Model
//!
//! This module defines the **symbol model** produced from a parsed source
//! file.  It captures exactly the information needed to answer "what does
//! this file bring into scope and what does it export to the rest of the
//! bundle?".
//!
//! ## Design Decisions
//!
//! * **Imports** — every `use` declaration is converted into one
#![allow(dead_code, unused_imports)]
//!   [`ImportedName`], preserving the source path, the imported symbol name,
//!   and the optional local alias.
//!
//! * **Exports** — only definitions marked `export` appear in the model.
//!   Each `export define` statement is represented by one [`ExportedDef`]
//!   variant that captures the kind-specific metadata (fields for structs,
//!   member names for enums, etc.).
//!
//! * **Trait implementations** — `implement Trait for Type` blocks are
//!   recorded as [`ExportedDef::TraitImpl`].  The function list is **not**
//!   expanded: the trait name already identifies the full method set, so
//!   repeating it here would be redundant.  Plain `implement for Type`
//!   blocks without a named trait expose their definitions individually (as
//!   [`ExportedDef::InherentImpl`]).
//!
//! * **No spans** — positions are intentionally omitted.  The symbol model
//!   is a pure semantic summary; callers that need source locations should
//!   consult the [`crate::syntax::SyntaxResult`] directly.

use serde::{Deserialize, Serialize};
use crate::syntax::SyntaxResult;
use crate::syntax::ast::{
    Chunk, DefExpr, TopItem, UseDecl, UseTail, Visibility,
};

// ---------------------------------------------------------------------------
// Imported names
// ---------------------------------------------------------------------------

/// A single name imported into the file's local scope via a `use` declaration.
///
/// # Examples
///
/// ```nova
/// use Std.Collections;          -- path = ["Std", "Collections"], name = "Collections", alias = None
/// use Std.IO as StdIO;          -- path = ["Std", "IO"], name = "IO",          alias = Some("StdIO")
/// use Std.Math.{sin, cos};      -- two ImportedName records, name = "sin" / "cos"
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportedName {
    /// The bundle/namespace path segments before the imported name.
    /// For `use Std.Collections.{List}` this is `["Std", "Collections"]`
    /// and `name` is `"List"`.
    pub path: Vec<String>,
    /// The original name as written in the source.
    pub name: String,
    /// Local alias if the import was `use … as Alias` or `use ….{name as Alias}`.
    pub alias: Option<String>,
}

// ---------------------------------------------------------------------------
// Exported definitions
// ---------------------------------------------------------------------------

/// A field in an exported struct.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedField {
    pub name: String,
    /// Textual representation of the field's type (dot-joined `TypeName`).
    pub type_name: String,
}

/// A single parameter in a function/lambda signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedParam {
    pub name: String,
    /// `None` when the parameter has no explicit type annotation.
    pub type_name: Option<String>,
}

/// A function signature captured from a top-level `export define` whose
/// body is a lambda expression.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedFunctionSig {
    pub params: Vec<ExportedParam>,
    /// Return type annotation, if present.
    pub return_type: Option<String>,
}

/// A member of an exported enum definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedEnumMember {
    pub name: String,
    /// Base type of the enum (dot-joined `TypeName`).
    pub base_type: String,
}

/// A case of an exported variant definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedVariantCase {
    pub name: String,
    pub type_name: String,
}

/// One entry in a trait definition's signature list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportedTraitSig {
    pub name: String,
    pub params: Vec<ExportedParam>,
    pub return_type: String,
}

/// An exported definition produced from a top-level `export define` or from
/// a method inside an inherent `implement for Type` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExportedDef {
    /// A plain value or constant (`export define name = expr`).
    Value {
        name: String,
        /// Explicit type annotation if the definition carried one.
        type_annotation: Option<String>,
    },

    /// A function / lambda (`export define name(…): Ret …`).
    Function {
        name: String,
        signature: ExportedFunctionSig,
        /// Explicit return-type annotation on the definition itself, if any.
        type_annotation: Option<String>,
    },

    /// A struct type definition.
    Struct {
        name: String,
        fields: Vec<ExportedField>,
    },

    /// An enum type definition.
    Enum {
        name: String,
        /// The enum's underlying type (dot-joined).
        base_type: String,
        members: Vec<String>,
    },

    /// A variant (sum-type) definition.
    Variant {
        name: String,
        cases: Vec<ExportedVariantCase>,
    },

    /// A trait type definition.
    Trait {
        name: String,
        signatures: Vec<ExportedTraitSig>,
    },

    /// A trait implementation block (`implement Trait for Type`).
    ///
    /// The function list is **not** expanded here because the trait name
    /// already uniquely identifies the required method set.
    TraitImpl {
        /// Dot-joined name of the trait being implemented.
        trait_name: String,
        /// Dot-joined name of the target type.
        target_type: String,
    },

    /// An inherent implementation block (`implement for Type`) — its
    /// exported methods are lifted into this record.
    InherentImpl {
        /// Dot-joined name of the target type.
        target_type: String,
        /// Exported method definitions from the block.
        methods: Vec<ExportedDef>,
    },
}

// ---------------------------------------------------------------------------
// Bundle exports – the top-level symbol model
// ---------------------------------------------------------------------------

/// The complete symbol model for a single compilation unit.
///
/// This is the output of the `"symbol"` incremental stage and is sufficient
/// to resolve cross-file references within a bundle without re-parsing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BundleExports {
    /// The namespace declared in this file (dot-joined path segments).
    pub namespace: String,
    /// All names imported by `use` declarations in this file.
    pub imports: Vec<ImportedName>,
    /// All definitions exported from this file.
    pub exports: Vec<ExportedDef>,
}

// ---------------------------------------------------------------------------
// Extraction helpers (private)
// ---------------------------------------------------------------------------

fn join_path(parts: &[crate::syntax::ast::Name]) -> String {
    parts.iter().map(|n| n.value.as_str()).collect::<Vec<_>>().join(".")
}

fn type_name_str(tn: &crate::syntax::ast::TypeName) -> String {
    join_path(&tn.parts)
}

fn extract_imports(uses: &[UseDecl]) -> Vec<ImportedName> {
    let mut out = Vec::new();
    for decl in uses {
        // The path up to (but not including) the final name.
        match &decl.tail {
            // `use A.B.{x, y}` or `use A.B.{x as z}`
            Some(UseTail::Selector(items)) => {
                let path: Vec<String> =
                    decl.path.iter().map(|n| n.value.clone()).collect();
                for item in items {
                    out.push(ImportedName {
                        path: path.clone(),
                        name: item.name.value.clone(),
                        alias: item.alias.as_ref().map(|a| a.value.clone()),
                    });
                }
            }
            // `use A.B as C`
            Some(UseTail::Alias(alias)) => {
                let mut path: Vec<String> =
                    decl.path.iter().map(|n| n.value.clone()).collect();
                // The last segment is the imported name; everything before is the path.
                let name = path.pop().unwrap_or_default();
                out.push(ImportedName {
                    path,
                    name,
                    alias: Some(alias.value.clone()),
                });
            }
            // `use A.B.C` — last segment is the name
            None => {
                let mut path: Vec<String> =
                    decl.path.iter().map(|n| n.value.clone()).collect();
                let name = path.pop().unwrap_or_default();
                out.push(ImportedName { path, name, alias: None });
            }
        }
    }
    out
}

fn extract_param(p: &crate::syntax::ast::Param) -> ExportedParam {
    ExportedParam {
        name: p.name.value.clone(),
        type_name: p.type_spec.as_ref().map(|ts| type_name_str(&ts.ty)),
    }
}

/// Try to interpret the body of a `define` as a function/lambda and produce
/// an [`ExportedFunctionSig`].  Returns `None` for non-lambda expressions.
fn extract_function_sig(expr: &DefExpr) -> Option<ExportedFunctionSig> {
    if let DefExpr::Exp(exp) = expr {
        if let crate::syntax::ast::Exp::Lambda(lam) = exp {
            let params = lam.params.iter().map(extract_param).collect();
            let return_type = Some(type_name_str(&lam.return_type.ty));
            return Some(ExportedFunctionSig { params, return_type });
        }
    }
    None
}

fn extract_definition(
    def: &crate::syntax::ast::Definition,
) -> Option<ExportedDef> {
    let name = def.name.value.clone();
    let type_annotation = def.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));

    match &def.expr {
        DefExpr::Struct(s) => {
            let fields = s
                .fields
                .iter()
                .map(|f| ExportedField {
                    name: f.name.value.clone(),
                    type_name: type_name_str(&f.type_spec.ty),
                })
                .collect();
            Some(ExportedDef::Struct { name, fields })
        }
        DefExpr::Enum(e) => {
            let base_type = type_name_str(&e.type_spec.ty);
            let members = e.members.iter().map(|m| m.name.value.clone()).collect();
            Some(ExportedDef::Enum { name, base_type, members })
        }
        DefExpr::Variant(v) => {
            let cases = v
                .members
                .iter()
                .map(|m| ExportedVariantCase {
                    name: m.name.value.clone(),
                    type_name: type_name_str(&m.type_spec.ty),
                })
                .collect();
            Some(ExportedDef::Variant { name, cases })
        }
        DefExpr::Trait(t) => {
            let signatures = t
                .sigs
                .iter()
                .map(|sig| ExportedTraitSig {
                    name: sig.name.value.clone(),
                    params: sig.params.iter().map(extract_param).collect(),
                    return_type: type_name_str(&sig.return_type.ty),
                })
                .collect();
            Some(ExportedDef::Trait { name, signatures })
        }
        DefExpr::Exp(_) => {
            // Distinguish lambda (function) from plain value.
            if let Some(sig) = extract_function_sig(&def.expr) {
                Some(ExportedDef::Function { name, signature: sig, type_annotation })
            } else {
                Some(ExportedDef::Value { name, type_annotation })
            }
        }
    }
}

