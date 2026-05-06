//! # Bundle Intermediate Format
//!
//! Defines the in-memory representation of the Nova bundle intermediate format
//! and the logic for extracting bundle data from parsed source files.
//!
//! ## File Format (NVIL binary)
//!
//! ```text
//! [Header 16 bytes]  ── Magic b"NVIL" + version (4×u16)
//! [Section 1] Metadata            — key-value extensible metadata
//! [Section 2] Dependencies        — ordered bundle deps with anchored versions
//! [Section 3] FileList            — relative file paths, sorted
//! [Section 4] TypeList            — pre-deduplicated type reference table
//! [Section 5] TypeDeclarations    — type declarations with TypeBody enum
//! [Section 6] ConstList           — pre-deduplicated constant reference table
//! [Section 7] ConstDefinitions    — constant values with file refs & spans
//! [Section 8] FuncList            — free-form function reference table
//! [Section 9] FuncBodies          — func hulls + semantic body models
//! [Section 10] ImplBodies         — impl blocks with method & const bodies
//! ```

pub mod encode;
pub mod decode;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Source location span
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl From<&crate::lexical::Span> for SourceSpan {
    fn from(s: &crate::lexical::Span) -> Self {
        SourceSpan {
            start_byte: s.start.byte() as u32,
            end_byte: s.end.byte() as u32,
            start_line: s.start.line() as u32,
            start_col: s.start.column() as u32,
            end_line: s.end.line() as u32,
            end_col: s.end.column() as u32,
        }
    }
}

// ---------------------------------------------------------------------------
// Format version
// ---------------------------------------------------------------------------

/// 64-bit semantic version: 16 bits per field (major.minor.patch.extra).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormatVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub extra: u16,
}

impl FormatVersion {
    pub const V1: FormatVersion = FormatVersion {
        major: 1, minor: 0, patch: 0, extra: 0,
    };
}

// ---------------------------------------------------------------------------
// Section 2 – Metadata
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub key: String,
    pub value: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Section 3 – Dependencies
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    pub anchor_mask: u64,
    pub version: FormatVersion,
    pub options: Vec<MetadataEntry>,
}

// ---------------------------------------------------------------------------
// Section 4 – Type list (pre-deduplicated table)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeListEntry {
    pub bundle_id: u32,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Section 5 – Type declarations
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TypeBody {
    Struct { fields: Vec<FieldDecl> },
    Enum { base_type: u32, members: Vec<String> },
    Variant { cases: Vec<VariantCaseDecl> },
    Trait { signatures: Vec<TraitSigDecl> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariantCaseDecl {
    pub name: String,
    pub type_index: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraitSigDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub type_index: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDecl {
    pub name: String,
    pub type_index: u32,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeDeclaration {
    pub type_index: u32,
    pub file_index: u32,
    pub span: SourceSpan,
    pub body: TypeBody,
}

// ---------------------------------------------------------------------------
// Section 6 – Constant list (pre-deduplicated table)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstListEntry {
    pub bundle_id: u32,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Section 7 – Constant definitions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstDefinition {
    pub const_index: u32,
    pub file_index: u32,
    pub span: SourceSpan,
    pub type_index: Option<u32>,
    pub expr: SemanticExpr,
}

// ---------------------------------------------------------------------------
// Semantic model for function bodies
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SemanticUnaryOp { Neg, Not, Len, BitNot }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SemanticBinaryOp {
    Add, Sub, Mul, Div, FloorDiv, Mod, Pow,
    Eq, NotEq, Less, LessEq, Greater, GreaterEq,
    And, Or, Concat,
    BitAnd, BitOr, BitXor, Shl, Shr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticParam {
    pub name: String,
    pub type_index: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticLocal {
    pub name: String,
    pub type_index: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticBlock {
    pub locals: Vec<SemanticLocal>,
    pub stmts: Vec<SemanticStmt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SemanticStmt {
    Empty,
    VarDecl { name: String, type_index: Option<u32>, init: Option<Box<SemanticExpr>>, is_var: bool },
    Assign { targets: Vec<SemanticExpr>, value: SemanticExpr },
    Call { expr: SemanticExpr },
    Do(SemanticBlock),
    While { cond: SemanticExpr, body: SemanticBlock },
    Repeat { body: SemanticBlock, until: SemanticExpr },
    If {
        cond: SemanticExpr,
        then: SemanticBlock,
        else_ifs: Vec<(SemanticExpr, SemanticBlock)>,
        else_block: Option<SemanticBlock>,
    },
    ForNumeric { var: String, start: SemanticExpr, end: SemanticExpr, step: Option<SemanticExpr>, body: SemanticBlock },
    ForGeneric { vars: Vec<String>, iter: SemanticExpr, body: SemanticBlock },
    Break,
    Continue,
    Return { values: Vec<SemanticExpr> },
    Label(String),
    Goto(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SemanticExpr {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Name(String),
    Field { object: Box<SemanticExpr>, field: String },
    Index { object: Box<SemanticExpr>, index: Box<SemanticExpr> },
    Call { func: Box<SemanticExpr>, args: Vec<SemanticExpr> },
    Unary { op: SemanticUnaryOp, expr: Box<SemanticExpr> },
    Binary { op: SemanticBinaryOp, left: Box<SemanticExpr>, right: Box<SemanticExpr> },
    Lambda { params: Vec<SemanticParam>, return_type: Option<u32>, body: SemanticBlock },
    VarDecl { name: String, type_index: Option<u32>, init: Option<Box<SemanticExpr>>, is_var: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticFuncBody {
    pub params: Vec<SemanticParam>,
    pub return_type: Option<u32>,
    pub locals: Vec<SemanticLocal>,
    pub body: SemanticBlock,
}

// ---------------------------------------------------------------------------
// Section 8 – Function list (pre-deduplicated table)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FuncListEntry {
    pub bundle_id: u32,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Section 9 – Function bodies
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FuncBody {
    pub func_index: u32,
    pub file_index: u32,
    pub span: SourceSpan,
    pub semantic: SemanticFuncBody,
}

// ---------------------------------------------------------------------------
// Section 10 – Impl bodies
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImplBody {
    pub type_index: u32,
    pub trait_index: Option<u32>,
    pub file_index: u32,
    pub span: SourceSpan,
    pub methods: Vec<FuncBody>,
}

// ---------------------------------------------------------------------------
// Top-level bundle
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub version: FormatVersion,
    pub metadata: Vec<MetadataEntry>,
    pub dependencies: Vec<Dependency>,
    pub files: Vec<String>,
    pub type_list: Vec<TypeListEntry>,
    pub type_declarations: Vec<TypeDeclaration>,
    pub const_list: Vec<ConstListEntry>,
    pub const_definitions: Vec<ConstDefinition>,
    pub func_list: Vec<FuncListEntry>,
    pub func_bodies: Vec<FuncBody>,
    pub impl_bodies: Vec<ImplBody>,
}

// ---------------------------------------------------------------------------
// Per-file fragment (pipeline intermediate)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BundleFragment {
    pub path: String,
    pub namespace: String,
    pub type_declarations: Vec<FragmentTypeDecl>,
    pub functions: Vec<FragmentFunc>,
    pub impl_blocks: Vec<FragmentImplBlock>,
    pub constants: Vec<FragmentConstDecl>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FragmentTypeBody {
    Struct { fields: Vec<FragmentField> },
    Enum { base_type: String, members: Vec<String> },
    Variant { cases: Vec<FragmentVariantCase> },
    Trait { signatures: Vec<FragmentTraitSig> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentTypeDecl {
    pub name: String,
    pub span: SourceSpan,
    pub body: FragmentTypeBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentVariantCase {
    pub name: String,
    pub type_name: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentTraitSig {
    pub name: String,
    pub params: Vec<FragmentParam>,
    pub return_type: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentField {
    pub name: String,
    pub type_name: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentFunc {
    pub name: String,
    pub span: SourceSpan,
    pub body: Option<FragmentFuncBody>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentImplBlock {
    pub trait_name: Option<String>,
    pub target_type: String,
    pub span: SourceSpan,
    pub method_bodies: Vec<FragmentFunc>,
    pub constant_defs: Vec<FragmentConstDecl>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentParam {
    pub name: String,
    pub type_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentLocal {
    pub name: String,
    pub type_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentFuncBody {
    pub params: Vec<FragmentParam>,
    pub return_type: Option<String>,
    pub locals: Vec<FragmentLocal>,
    pub body: FragmentBlock,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentBlock {
    pub locals: Vec<FragmentLocal>,
    pub stmts: Vec<FragmentStmt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FragmentStmt {
    Empty,
    VarDecl { name: String, type_name: Option<String>, init: Option<Box<FragmentExpr>>, is_var: bool },
    Assign { targets: Vec<FragmentExpr>, value: FragmentExpr },
    Call { expr: FragmentExpr },
    Do(FragmentBlock),
    While { cond: FragmentExpr, body: FragmentBlock },
    Repeat { body: FragmentBlock, until: FragmentExpr },
    If {
        cond: FragmentExpr,
        then: FragmentBlock,
        else_ifs: Vec<(FragmentExpr, FragmentBlock)>,
        else_block: Option<FragmentBlock>,
    },
    ForNumeric { var: String, start: FragmentExpr, end: FragmentExpr, step: Option<FragmentExpr>, body: FragmentBlock },
    ForGeneric { vars: Vec<String>, iter: FragmentExpr, body: FragmentBlock },
    Break,
    Continue,
    Return { values: Vec<FragmentExpr> },
    Label(String),
    Goto(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FragmentExpr {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Name(String),
    Field { object: Box<FragmentExpr>, field: String },
    Index { object: Box<FragmentExpr>, index: Box<FragmentExpr> },
    Call { func: Box<FragmentExpr>, args: Vec<FragmentExpr> },
    Unary { op: SemanticUnaryOp, expr: Box<FragmentExpr> },
    Binary { op: SemanticBinaryOp, left: Box<FragmentExpr>, right: Box<FragmentExpr> },
    Lambda { params: Vec<FragmentParam>, return_type: Option<String>, body: FragmentBlock },
    VarDecl { name: String, type_name: Option<String>, init: Option<Box<FragmentExpr>>, is_var: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FragmentConstDecl {
    pub name: String,
    pub span: SourceSpan,
    pub type_name: Option<String>,
    pub expr: FragmentExpr,
}

// ---------------------------------------------------------------------------
// Extraction: ParseOutput → BundleFragment
// ---------------------------------------------------------------------------

use crate::syntax::SyntaxResult;
use crate::syntax::ast::{
    ArgsKind, Block, DefExpr, Exp, Stat, TopItem,
    VarDeclKind,
};

fn join_path(parts: &[crate::syntax::ast::Name]) -> String {
    parts.iter().map(|n| n.value.as_str()).collect::<Vec<_>>().join(".")
}

fn type_name_str(tn: &crate::syntax::ast::TypeName) -> String {
    join_path(&tn.parts)
}

// ── Semantic extraction helpers ─────────────────────────────────────────

fn extract_lambda_body(lambda: &crate::syntax::ast::ExpLambda, ns: &str) -> FragmentFuncBody {
    let params = lambda.params.iter().map(|p| {
        let type_name = p.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));
        FragmentParam { name: p.name.value.clone(), type_name }
    }).collect();
    let return_type = Some(type_name_str(&lambda.return_type.ty));
    let body = extract_ast_block(&lambda.block, ns);
    let all_locals = collect_all_locals(&body);
    FragmentFuncBody { params, return_type, locals: all_locals, body }
}

fn collect_all_locals(block: &FragmentBlock) -> Vec<FragmentLocal> {
    let mut acc = Vec::new();
    collect_locals_rec(block, &mut acc);
    let mut deduped = Vec::new();
    for l in acc {
        if !deduped.iter().any(|x: &FragmentLocal| x.name == l.name) {
            deduped.push(l);
        }
    }
    deduped
}

fn collect_locals_rec(block: &FragmentBlock, acc: &mut Vec<FragmentLocal>) {
    for l in &block.locals {
        if !acc.iter().any(|x: &FragmentLocal| x.name == l.name) {
            acc.push(l.clone());
        }
    }
    for stmt in &block.stmts {
        match stmt {
            FragmentStmt::Do(b)
            | FragmentStmt::While { body: b, .. }
            | FragmentStmt::Repeat { body: b, .. } => {
                collect_locals_rec(b, acc);
            }
            FragmentStmt::If { then, else_ifs, else_block, .. } => {
                collect_locals_rec(then, acc);
                for (_, eb) in else_ifs { collect_locals_rec(eb, acc); }
                if let Some(eb) = else_block { collect_locals_rec(eb, acc); }
            }
            FragmentStmt::ForNumeric { body: b, .. }
            | FragmentStmt::ForGeneric { body: b, .. } => {
                collect_locals_rec(b, acc);
            }
            _ => {}
        }
    }
}

fn extract_ast_block(block: &Block, ns: &str) -> FragmentBlock {
    let mut stmts = Vec::new();
    let mut locals = Vec::new();
    for stat in &block.stats {
        let fs = extract_ast_stmt(stat, ns);
        if let FragmentStmt::VarDecl { ref name, ref type_name, .. } = fs {
            if !locals.iter().any(|l: &FragmentLocal| l.name == *name) {
                locals.push(FragmentLocal {
                    name: name.clone(),
                    type_name: type_name.clone(),
                });
            }
        }
        stmts.push(fs);
    }
    FragmentBlock { locals, stmts }
}

fn extract_ast_stmt(stmt: &Stat, ns: &str) -> FragmentStmt {
    match stmt {
        Stat::Empty(_) => FragmentStmt::Empty,
        Stat::Assign(assign) => {
            if assign.vars.len() == 1 {
                if let Exp::VarDecl(vd) = &assign.vars[0] {
                    let type_name = vd.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));
                    let init = if assign.exprs.is_empty() {
                        None
                    } else {
                        Some(Box::new(extract_ast_expr(&assign.exprs[0], ns)))
                    };
                    let is_var = matches!(vd.kind, VarDeclKind::Var);
                    return FragmentStmt::VarDecl {
                        name: vd.name.value.clone(),
                        type_name,
                        init,
                        is_var,
                    };
                }
            }
            let targets = assign.vars.iter().map(|e| extract_ast_expr(e, ns)).collect();
            let value = if assign.exprs.is_empty() {
                FragmentExpr::Nil
            } else {
                extract_ast_expr(&assign.exprs[0], ns)
            };
            FragmentStmt::Assign { targets, value }
        }
        Stat::Call(sc) => {
            FragmentStmt::Call { expr: extract_ast_expr(&sc.call, ns) }
        }
        Stat::Do(sd) => {
            FragmentStmt::Do(extract_ast_block(&sd.block, ns))
        }
        Stat::While(sw) => {
            let cond = extract_ast_expr(&sw.cond, ns);
            let body = extract_ast_block(&sw.block, ns);
            FragmentStmt::While { cond, body }
        }
        Stat::Repeat(sr) => {
            let body = extract_ast_block(&sr.block, ns);
            let until = extract_ast_expr(&sr.cond, ns);
            FragmentStmt::Repeat { body, until }
        }
        Stat::If(si) => {
            let cond = extract_ast_expr(&si.clauses[0].cond, ns);
            let then = extract_ast_block(&si.clauses[0].block, ns);
            let else_ifs = si.clauses[1..].iter().map(|clause| {
                (extract_ast_expr(&clause.cond, ns), extract_ast_block(&clause.block, ns))
            }).collect();
            let else_block = si.else_block.as_ref().map(|b| extract_ast_block(b, ns));
            FragmentStmt::If { cond, then, else_ifs, else_block }
        }
        Stat::ForNumeric(sfn) => {
            let var = sfn.name.value.clone();
            let start = extract_ast_expr(&sfn.start, ns);
            let end = extract_ast_expr(&sfn.end, ns);
            let step = sfn.step.as_ref().map(|e| extract_ast_expr(e, ns));
            let body = extract_ast_block(&sfn.block, ns);
            FragmentStmt::ForNumeric { var, start, end, step, body }
        }
        Stat::ForGeneric(sfg) => {
            let vars = sfg.names.iter().map(|n| n.value.clone()).collect();
            let iter = extract_ast_expr(&sfg.exprs[0], ns);
            let body = extract_ast_block(&sfg.block, ns);
            FragmentStmt::ForGeneric { vars, iter, body }
        }
        Stat::Break(_) => FragmentStmt::Break,
        Stat::Continue(_) => FragmentStmt::Continue,
        Stat::Return(sr) => {
            let values = sr.exprs.iter().map(|e| extract_ast_expr(e, ns)).collect();
            FragmentStmt::Return { values }
        }
        Stat::Goto(sg) => FragmentStmt::Goto(sg.label.value.clone()),
        Stat::Label(sl) => FragmentStmt::Label(sl.label.value.clone()),
    }
}

fn extract_ast_expr(exp: &crate::syntax::ast::Exp, ns: &str) -> FragmentExpr {
    match exp {
        Exp::Nil(_) => FragmentExpr::Nil,
        Exp::Bool(b) => FragmentExpr::Bool(b.value),
        Exp::Number(n) => FragmentExpr::Number(n.value.clone()),
        Exp::String(s) => FragmentExpr::String(s.value.clone()),
        Exp::Name(n) => FragmentExpr::Name(n.name.clone()),
        Exp::Paren(p) => extract_ast_expr(&p.inner, ns),
        Exp::Field(f) => FragmentExpr::Field {
            object: Box::new(extract_ast_expr(&f.prefix, ns)),
            field: f.name.value.clone(),
        },
        Exp::Index(idx) => FragmentExpr::Index {
            object: Box::new(extract_ast_expr(&idx.prefix, ns)),
            index: Box::new(extract_ast_expr(&idx.index, ns)),
        },
        Exp::Call(c) => {
            let args = match &c.args.kind {
                ArgsKind::ExpList(list) => {
                    list.iter().map(|e| extract_ast_expr(e, ns)).collect()
                }
                ArgsKind::Initializer(init) => {
                    init.fields.iter().map(|f| extract_ast_expr(&f.value, ns)).collect()
                }
            };
            FragmentExpr::Call {
                func: Box::new(extract_ast_expr(&c.prefix, ns)),
                args,
            }
        }
        Exp::VarDecl(vd) => {
            let type_name = vd.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));
            let is_var = matches!(vd.kind, VarDeclKind::Var);
            FragmentExpr::VarDecl {
                name: vd.name.value.clone(),
                type_name,
                init: None,
                is_var,
            }
        }
        Exp::Lambda(lam) => {
            let params = lam.params.iter().map(|p| {
                let tn = p.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));
                FragmentParam { name: p.name.value.clone(), type_name: tn }
            }).collect();
            let rt = Some(type_name_str(&lam.return_type.ty));
            let body = extract_ast_block(&lam.block, ns);
            FragmentExpr::Lambda { params, return_type: rt, body }
        }
        Exp::Unary(u) => {
            use crate::syntax::ast::UnOp;
            let op = match u.op {
                UnOp::Neg => SemanticUnaryOp::Neg,
                UnOp::Not => SemanticUnaryOp::Not,
                UnOp::Len => SemanticUnaryOp::Len,
                UnOp::BitNot => SemanticUnaryOp::BitNot,
            };
            FragmentExpr::Unary {
                op,
                expr: Box::new(extract_ast_expr(&u.exp, ns)),
            }
        }
        Exp::Binary(b) => {
            use crate::syntax::ast::BinOp;
            let left = extract_ast_expr(&b.left, ns);
            let right = extract_ast_expr(&b.right, ns);
            let op = match b.op {
                BinOp::Or => SemanticBinaryOp::Or,
                BinOp::And => SemanticBinaryOp::And,
                BinOp::Eq => SemanticBinaryOp::Eq,
                BinOp::NotEq => SemanticBinaryOp::NotEq,
                BinOp::Less => SemanticBinaryOp::Less,
                BinOp::LessEq => SemanticBinaryOp::LessEq,
                BinOp::Greater => SemanticBinaryOp::Greater,
                BinOp::GreaterEq => SemanticBinaryOp::GreaterEq,
                BinOp::BitOr => SemanticBinaryOp::BitOr,
                BinOp::BitXor => SemanticBinaryOp::BitXor,
                BinOp::BitAnd => SemanticBinaryOp::BitAnd,
                BinOp::ShiftLeft => SemanticBinaryOp::Shl,
                BinOp::ShiftRight => SemanticBinaryOp::Shr,
                BinOp::Concat => SemanticBinaryOp::Concat,
                BinOp::Add => SemanticBinaryOp::Add,
                BinOp::Sub => SemanticBinaryOp::Sub,
                BinOp::Mul => SemanticBinaryOp::Mul,
                BinOp::Div => SemanticBinaryOp::Div,
                BinOp::FloorDiv => SemanticBinaryOp::FloorDiv,
                BinOp::Mod => SemanticBinaryOp::Mod,
                BinOp::Pow => SemanticBinaryOp::Pow,
            };
            FragmentExpr::Binary { op, left: Box::new(left), right: Box::new(right) }
        }
    }
}

/// Extract a per-file [`BundleFragment`] from a parsed source file.
pub fn extract_fragment(path: &str, result: &SyntaxResult) -> BundleFragment {
    let chunk = &result.chunk;
    let namespace = join_path(&chunk.namespace.path);

    let mut type_declarations = Vec::new();
    let mut functions = Vec::new();
    let mut impl_blocks = Vec::new();
    let mut constants = Vec::new();

    for item in &chunk.items {
        match item {
            TopItem::Definition(def) => {
                let fqn = qualify(&namespace, &def.name.value);
                match &def.expr {
                    DefExpr::Struct(s) => {
                        let fields = s.fields.iter()
                            .map(|f| FragmentField {
                                name: f.name.value.clone(),
                                type_name: type_name_str(&f.type_spec.ty),
                                span: SourceSpan::from(&f.span),
                            })
                            .collect();
                        type_declarations.push(FragmentTypeDecl {
                            name: fqn,
                            span: SourceSpan::from(&def.span),
                            body: FragmentTypeBody::Struct { fields },
                        });
                    }
                    DefExpr::Enum(e) => {
                        let base_type = type_name_str(&e.type_spec.ty);
                        let members = e.members.iter()
                            .map(|m| m.name.value.clone())
                            .collect();
                        type_declarations.push(FragmentTypeDecl {
                            name: fqn,
                            span: SourceSpan::from(&def.span),
                            body: FragmentTypeBody::Enum { base_type, members },
                        });
                    }
                    DefExpr::Variant(v) => {
                        let cases = v.members.iter()
                            .map(|m| FragmentVariantCase {
                                name: m.name.value.clone(),
                                type_name: type_name_str(&m.type_spec.ty),
                                span: SourceSpan::from(&m.span),
                            })
                            .collect();
                        type_declarations.push(FragmentTypeDecl {
                            name: fqn,
                            span: SourceSpan::from(&def.span),
                            body: FragmentTypeBody::Variant { cases },
                        });
                    }
                    DefExpr::Trait(t) => {
                        let signatures = t.sigs.iter()
                            .map(|sig| FragmentTraitSig {
                                name: sig.name.value.clone(),
                                params: sig.params.iter().map(|p| {
                                    let tn = p.type_spec.as_ref().map(|ts| type_name_str(&ts.ty));
                                    FragmentParam { name: p.name.value.clone(), type_name: tn }
                                }).collect(),
                                return_type: type_name_str(&sig.return_type.ty),
                                span: SourceSpan::from(&sig.span),
                            })
                            .collect();
                        type_declarations.push(FragmentTypeDecl {
                            name: fqn,
                            span: SourceSpan::from(&def.span),
                            body: FragmentTypeBody::Trait { signatures },
                        });
                    }
                    DefExpr::Exp(exp) => {
                        if let crate::syntax::ast::Exp::Lambda(lam) = exp {
                            let body = extract_lambda_body(lam, &namespace);
                            functions.push(FragmentFunc {
                                name: fqn,
                                span: SourceSpan::from(&def.span),
                                body: Some(body),
                            });
                        } else {
                            let expr = extract_ast_expr(exp, &namespace);
                            constants.push(FragmentConstDecl {
                                name: fqn,
                                span: SourceSpan::from(&def.span),
                                type_name: def.type_spec.as_ref().map(|ts| type_name_str(&ts.ty)),
                                expr,
                            });
                        }
                    }
                }
            }
            TopItem::Implementation(imp) => {
                let target_fqn = qualify_type_name(&namespace, &imp.target);
                let trait_name = imp.trait_type.as_ref().map(|tn| qualify_type_name(&namespace, tn));
                let mut method_bodies = Vec::new();
                let mut constant_defs = Vec::new();
                for def in &imp.items {
                    let item_fqn = format!("{}.{}", target_fqn, def.name.value);
                    match &def.expr {
                        DefExpr::Exp(exp) if matches!(exp, crate::syntax::ast::Exp::Lambda(_)) => {
                            if let crate::syntax::ast::Exp::Lambda(lam) = exp {
                                let body = extract_lambda_body(lam, &namespace);
                                method_bodies.push(FragmentFunc {
                                    name: item_fqn,
                                    span: SourceSpan::from(&def.span),
                                    body: Some(body),
                                });
                            }
                        }
                        DefExpr::Exp(exp) => {
                            let expr = extract_ast_expr(exp, &namespace);
                            constant_defs.push(FragmentConstDecl {
                                name: item_fqn,
                                span: SourceSpan::from(&def.span),
                                type_name: def.type_spec.as_ref().map(|ts| type_name_str(&ts.ty)),
                                expr,
                            });
                        }
                        _ => {
                            method_bodies.push(FragmentFunc {
                                name: item_fqn,
                                span: SourceSpan::from(&def.span),
                                body: None,
                            });
                        }
                    }
                }
                impl_blocks.push(FragmentImplBlock {
                    trait_name,
                    target_type: target_fqn,
                    span: SourceSpan::from(&imp.span),
                    method_bodies,
                    constant_defs,
                });
            }
        }
    }

    BundleFragment {
        path: path.to_string(),
        namespace,
        type_declarations,
        functions,
        impl_blocks,
        constants,
    }
}

fn qualify(ns: &str, name: &str) -> String {
    if ns.is_empty() { name.to_string() } else { format!("{}.{}", ns, name) }
}

/// Qualify a type reference with the file's namespace if it is a simple name
/// (single part). Multi-part names like `fmt.Display` are already qualified.
fn qualify_type_name(ns: &str, tn: &crate::syntax::ast::TypeName) -> String {
    let local = type_name_str(tn);
    if tn.parts.len() == 1 && !ns.is_empty() {
        format!("{}.{}", ns, local)
    } else {
        local
    }
}

// ---------------------------------------------------------------------------
// Assembly: Vec<BundleFragment> → Bundle
// ---------------------------------------------------------------------------

/// Assemble a complete [`Bundle`] from per-file fragments.
///
/// This function resolves type-name references to indices, builds the
/// deduplicated type and function tables, and produces the section 4–9 data.
pub fn assemble(fragments: Vec<BundleFragment>) -> Bundle {
    // --- Section 3: File list (sorted, deduped) ---
    let mut files: Vec<String> = fragments.iter().map(|f| f.path.clone()).collect();
    files.sort();
    files.dedup();

    let file_index = |path: &str| -> u32 {
        files.iter().position(|f| f == path).unwrap() as u32
    };

    // --- Section 4: Type list (collect all FQNs used) ---
    let mut type_names: Vec<String> = Vec::new();
    for frag in &fragments {
        for decl in &frag.type_declarations {
            push_unique(&mut type_names, &decl.name);
            match &decl.body {
                FragmentTypeBody::Struct { fields } => {
                    for field in fields {
                        push_unique(&mut type_names, &field.type_name);
                    }
                }
                FragmentTypeBody::Enum { base_type, members: _ } => {
                    push_unique(&mut type_names, base_type);
                }
                FragmentTypeBody::Variant { cases } => {
                    for case in cases {
                        push_unique(&mut type_names, &case.type_name);
                    }
                }
                FragmentTypeBody::Trait { signatures } => {
                    for sig in signatures {
                        push_unique(&mut type_names, &sig.return_type);
                        for param in &sig.params {
                            if let Some(ref tn) = param.type_name {
                                push_unique(&mut type_names, tn);
                            }
                        }
                    }
                }
            }
        }
        for block in &frag.impl_blocks {
            push_unique(&mut type_names, &block.target_type);
            if let Some(ref tn) = block.trait_name {
                push_unique(&mut type_names, tn);
            }
        }
        for c in &frag.constants {
            if let Some(ref tn) = c.type_name {
                push_unique(&mut type_names, tn);
            }
        }
        for block in &frag.impl_blocks {
            for c in &block.constant_defs {
                if let Some(ref tn) = c.type_name {
                    push_unique(&mut type_names, tn);
                }
            }
        }
        for func in &frag.functions {
            if let Some(body) = &func.body {
                for p in &body.params {
                    if let Some(ref tn) = p.type_name {
                        push_unique(&mut type_names, tn);
                    }
                }
                if let Some(ref ret) = body.return_type {
                    push_unique(&mut type_names, ret);
                }
                for l in &body.locals {
                    if let Some(ref tn) = l.type_name {
                        push_unique(&mut type_names, tn);
                    }
                }
            }
        }
        for block in &frag.impl_blocks {
            for method in &block.method_bodies {
                if let Some(body) = &method.body {
                    for p in &body.params {
                        if let Some(ref tn) = p.type_name {
                            push_unique(&mut type_names, tn);
                        }
                    }
                    if let Some(ref ret) = body.return_type {
                        push_unique(&mut type_names, ret);
                    }
                    for l in &body.locals {
                        if let Some(ref tn) = l.type_name {
                            push_unique(&mut type_names, tn);
                        }
                    }
                }
            }
        }
    }
    type_names.sort();

    let type_list: Vec<TypeListEntry> = type_names.iter()
        .map(|name| TypeListEntry { bundle_id: 0, name: name.clone() })
        .collect();

    let type_index = |name: &str| -> u32 {
        type_list.iter().position(|t| t.name == name).unwrap() as u32
    };

    // --- Section 5: Type declarations ---
    let mut type_declarations = Vec::new();
    for frag in &fragments {
        let fi = file_index(&frag.path);
        for decl in &frag.type_declarations {
            let ti = type_index(&decl.name);
            let body = match &decl.body {
                FragmentTypeBody::Struct { fields } => {
                    TypeBody::Struct {
                        fields: fields.iter().map(|f| FieldDecl {
                            name: f.name.clone(),
                            type_index: type_index(&f.type_name),
                            span: f.span.clone(),
                        }).collect(),
                    }
                }
                FragmentTypeBody::Enum { base_type, members } => {
                    TypeBody::Enum {
                        base_type: type_index(base_type),
                        members: members.clone(),
                    }
                }
                FragmentTypeBody::Variant { cases } => {
                    TypeBody::Variant {
                        cases: cases.iter().map(|c| VariantCaseDecl {
                            name: c.name.clone(),
                            type_index: type_index(&c.type_name),
                            span: c.span.clone(),
                        }).collect(),
                    }
                }
                FragmentTypeBody::Trait { signatures } => {
                    TypeBody::Trait {
                        signatures: signatures.iter().map(|s| TraitSigDecl {
                            name: s.name.clone(),
                            params: s.params.iter().map(|p| Param {
                                name: p.name.clone(),
                                type_index: p.type_name.as_ref().map(|tn| type_index(tn)),
                            }).collect(),
                            return_type: type_index(&s.return_type),
                            span: s.span.clone(),
                        }).collect(),
                    }
                }
            };
            type_declarations.push(TypeDeclaration {
                type_index: ti,
                file_index: fi,
                span: decl.span.clone(),
                body,
            });
        }
    }
    type_declarations.sort_by(|a, b| a.type_index.cmp(&b.type_index));

    // --- Section 6: Constant list (collect all FQNs used) ---
    let mut const_names: Vec<String> = Vec::new();
    for frag in &fragments {
        for c in &frag.constants {
            push_unique(&mut const_names, &c.name);
        }
        for block in &frag.impl_blocks {
            for c in &block.constant_defs {
                push_unique(&mut const_names, &c.name);
            }
        }
    }
    const_names.sort();

    let const_list: Vec<ConstListEntry> = const_names.iter()
        .map(|name| ConstListEntry { bundle_id: 0, name: name.clone() })
        .collect();

    let const_index = |name: &str| -> u32 {
        const_list.iter().position(|c| c.name == name).unwrap() as u32
    };

    // --- Section 7: Constant definitions ---
    let mut const_definitions = Vec::new();
    for frag in &fragments {
        let fi = file_index(&frag.path);
        for c in &frag.constants {
            const_definitions.push(ConstDefinition {
                const_index: const_index(&c.name),
                file_index: fi,
                span: c.span.clone(),
                type_index: c.type_name.as_ref().map(|tn| type_index(tn)),
                expr: resolve_expr(&c.expr, &type_index),
            });
        }
        for block in &frag.impl_blocks {
            for c in &block.constant_defs {
                const_definitions.push(ConstDefinition {
                    const_index: const_index(&c.name),
                    file_index: fi,
                    span: c.span.clone(),
                    type_index: c.type_name.as_ref().map(|tn| type_index(tn)),
                    expr: resolve_expr(&c.expr, &type_index),
                });
            }
        }
    }
    const_definitions.sort_by(|a, b| a.const_index.cmp(&b.const_index));

    // --- Section 8: Function list (collect all FQNs used) ---
    let mut func_names: Vec<String> = Vec::new();
    for frag in &fragments {
        for func in &frag.functions {
            push_unique(&mut func_names, &func.name);
        }
        for block in &frag.impl_blocks {
            for method in &block.method_bodies {
                push_unique(&mut func_names, &method.name);
            }
        }
    }
    func_names.sort();

    let func_list: Vec<FuncListEntry> = func_names.iter()
        .map(|name| FuncListEntry { bundle_id: 0, name: name.clone() })
        .collect();

    let func_index = |name: &str| -> u32 {
        func_list.iter().position(|f| f.name == name).unwrap() as u32
    };

    // --- Section 9: Function bodies ---
    let mut func_bodies = Vec::new();
    for frag in &fragments {
        let fi = file_index(&frag.path);
        for func in &frag.functions {
            let semantic = match &func.body {
                Some(body) => SemanticFuncBody {
                    params: body.params.iter().map(|p| SemanticParam {
                        name: p.name.clone(),
                        type_index: p.type_name.as_ref().map(|tn| type_index(tn)),
                    }).collect(),
                    return_type: body.return_type.as_ref().map(|tn| type_index(tn)),
                    locals: body.locals.iter().map(|l| SemanticLocal {
                        name: l.name.clone(),
                        type_index: l.type_name.as_ref().map(|tn| type_index(tn)),
                    }).collect(),
                    body: resolve_block(&body.body, &type_index),
                },
                None => SemanticFuncBody {
                    params: vec![],
                    return_type: None,
                    locals: vec![],
                    body: SemanticBlock { locals: vec![], stmts: vec![] },
                },
            };
            func_bodies.push(FuncBody {
                func_index: func_index(&func.name),
                file_index: fi,
                span: func.span.clone(),
                semantic,
            });
        }
    }
    func_bodies.sort_by(|a, b| a.func_index.cmp(&b.func_index));

    // --- Section 10: Impl bodies ---
    let mut impl_bodies = Vec::new();
    for frag in &fragments {
        let fi = file_index(&frag.path);
        for block in &frag.impl_blocks {
            let ti = type_index(&block.target_type);
            let trait_idx = block.trait_name.as_ref().map(|n| type_index(n));
            let methods: Vec<FuncBody> = block.method_bodies.iter()
                .map(|m| FuncBody {
                    func_index: func_index(&m.name),
                    file_index: fi,
                    span: m.span.clone(),
                    semantic: match &m.body {
                        Some(body) => SemanticFuncBody {
                            params: body.params.iter().map(|p| SemanticParam {
                                name: p.name.clone(),
                                type_index: p.type_name.as_ref().map(|tn| type_index(tn)),
                            }).collect(),
                            return_type: body.return_type.as_ref().map(|tn| type_index(tn)),
                            locals: body.locals.iter().map(|l| SemanticLocal {
                                name: l.name.clone(),
                                type_index: l.type_name.as_ref().map(|tn| type_index(tn)),
                            }).collect(),
                            body: resolve_block(&body.body, &type_index),
                        },
                        None => SemanticFuncBody {
                            params: vec![],
                            return_type: None,
                            locals: vec![],
                            body: SemanticBlock { locals: vec![], stmts: vec![] },
                        },
                    },
                })
                .collect();
            impl_bodies.push(ImplBody {
                type_index: ti,
                trait_index: trait_idx,
                file_index: fi,
                span: block.span.clone(),
                methods,
            });
        }
    }

    // --- Section 2: Metadata ---
    let metadata = vec![
        MetadataEntry {
            key: "com.nova.bundle.name".into(),
            value: b"default".to_vec(),
        },
        MetadataEntry {
            key: "com.nova.bundle.version".into(),
            value: b"1.0.0".to_vec(),
        },
        MetadataEntry {
            key: "com.nova.language.version".into(),
            value: b"0.1.0".to_vec(),
        },
        MetadataEntry {
            key: "com.nova.file.count".into(),
            value: files.len().to_string().into_bytes(),
        },
    ];

    Bundle {
        version: FormatVersion::V1,
        metadata,
        dependencies: vec![],
        files,
        type_list,
        type_declarations,
        const_list,
        const_definitions,
        func_list,
        func_bodies,
        impl_bodies,
    }
}

fn push_unique(vec: &mut Vec<String>, item: &str) {
    if !vec.iter().any(|x| x == item) {
        vec.push(item.to_string());
    }
}

// ── Fragment-to-Bundle resolution helpers ──────────────────────────────

fn resolve_block<F: Fn(&str) -> u32>(fb: &FragmentBlock, resolve_type: &F) -> SemanticBlock {
    let locals = fb.locals.iter().map(|l| SemanticLocal {
        name: l.name.clone(),
        type_index: l.type_name.as_ref().map(|tn| resolve_type(tn)),
    }).collect();
    let stmts = fb.stmts.iter().map(|s| resolve_stmt(s, resolve_type)).collect();
    SemanticBlock { locals, stmts }
}

fn resolve_stmt<F: Fn(&str) -> u32>(fs: &FragmentStmt, rt: &F) -> SemanticStmt {
    match fs {
        FragmentStmt::Empty => SemanticStmt::Empty,
        FragmentStmt::VarDecl { name, type_name, init, is_var } => {
            SemanticStmt::VarDecl {
                name: name.clone(),
                type_index: type_name.as_ref().map(|tn| rt(tn)),
                init: init.as_ref().map(|e| Box::new(resolve_expr(e, rt))),
                is_var: *is_var,
            }
        }
        FragmentStmt::Assign { targets, value } => SemanticStmt::Assign {
            targets: targets.iter().map(|e| resolve_expr(e, rt)).collect(),
            value: resolve_expr(value, rt),
        },
        FragmentStmt::Call { expr } => SemanticStmt::Call {
            expr: resolve_expr(expr, rt),
        },
        FragmentStmt::Do(b) => SemanticStmt::Do(resolve_block(b, rt)),
        FragmentStmt::While { cond, body } => SemanticStmt::While {
            cond: resolve_expr(cond, rt),
            body: resolve_block(body, rt),
        },
        FragmentStmt::Repeat { body, until } => SemanticStmt::Repeat {
            body: resolve_block(body, rt),
            until: resolve_expr(until, rt),
        },
        FragmentStmt::If { cond, then, else_ifs, else_block } => SemanticStmt::If {
            cond: resolve_expr(cond, rt),
            then: resolve_block(then, rt),
            else_ifs: else_ifs.iter().map(|(c, b)| (resolve_expr(c, rt), resolve_block(b, rt))).collect(),
            else_block: else_block.as_ref().map(|b| resolve_block(b, rt)),
        },
        FragmentStmt::ForNumeric { var, start, end, step, body } => SemanticStmt::ForNumeric {
            var: var.clone(),
            start: resolve_expr(start, rt),
            end: resolve_expr(end, rt),
            step: step.as_ref().map(|e| resolve_expr(e, rt)),
            body: resolve_block(body, rt),
        },
        FragmentStmt::ForGeneric { vars, iter, body } => SemanticStmt::ForGeneric {
            vars: vars.clone(),
            iter: resolve_expr(iter, rt),
            body: resolve_block(body, rt),
        },
        FragmentStmt::Break => SemanticStmt::Break,
        FragmentStmt::Continue => SemanticStmt::Continue,
        FragmentStmt::Return { values } => SemanticStmt::Return {
            values: values.iter().map(|e| resolve_expr(e, rt)).collect(),
        },
        FragmentStmt::Label(n) => SemanticStmt::Label(n.clone()),
        FragmentStmt::Goto(n) => SemanticStmt::Goto(n.clone()),
    }
}

fn resolve_expr<F: Fn(&str) -> u32>(fe: &FragmentExpr, rt: &F) -> SemanticExpr {
    match fe {
        FragmentExpr::Nil => SemanticExpr::Nil,
        FragmentExpr::Bool(b) => SemanticExpr::Bool(*b),
        FragmentExpr::Number(n) => SemanticExpr::Number(n.clone()),
        FragmentExpr::String(s) => SemanticExpr::String(s.clone()),
        FragmentExpr::Name(n) => SemanticExpr::Name(n.clone()),
        FragmentExpr::Field { object, field } => SemanticExpr::Field {
            object: Box::new(resolve_expr(object, rt)),
            field: field.clone(),
        },
        FragmentExpr::Index { object, index } => SemanticExpr::Index {
            object: Box::new(resolve_expr(object, rt)),
            index: Box::new(resolve_expr(index, rt)),
        },
        FragmentExpr::Call { func, args } => SemanticExpr::Call {
            func: Box::new(resolve_expr(func, rt)),
            args: args.iter().map(|a| resolve_expr(a, rt)).collect(),
        },
        FragmentExpr::Unary { op, expr } => SemanticExpr::Unary {
            op: op.clone(),
            expr: Box::new(resolve_expr(expr, rt)),
        },
        FragmentExpr::Binary { op, left, right } => SemanticExpr::Binary {
            op: op.clone(),
            left: Box::new(resolve_expr(left, rt)),
            right: Box::new(resolve_expr(right, rt)),
        },
        FragmentExpr::Lambda { params, return_type, body } => SemanticExpr::Lambda {
            params: params.iter().map(|p| SemanticParam {
                name: p.name.clone(),
                type_index: p.type_name.as_ref().map(|tn| rt(tn)),
            }).collect(),
            return_type: return_type.as_ref().map(|tn| rt(tn)),
            body: resolve_block(body, rt),
        },
        FragmentExpr::VarDecl { name, type_name, init, is_var } => SemanticExpr::VarDecl {
            name: name.clone(),
            type_index: type_name.as_ref().map(|tn| rt(tn)),
            init: init.as_ref().map(|e| Box::new(resolve_expr(e, rt))),
            is_var: *is_var,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical;
    use crate::syntax;

    fn parse(src: &str) -> SyntaxResult {
        let lex = lexical::transform(src).unwrap();
        syntax::transform(lex).unwrap()
    }

    // ── Fragment extraction tests ─────────────────────────────────────────

    #[test]
    fn fragment_empty_file() {
        let result = parse("namespace Empty;");
        let frag = extract_fragment("empty.nv", &result);
        assert_eq!(frag.path, "empty.nv");
        assert_eq!(frag.namespace, "Empty");
        assert!(frag.type_declarations.is_empty());
        assert!(frag.functions.is_empty());
        assert!(frag.impl_blocks.is_empty());
    }

    #[test]
    fn fragment_struct_declaration() {
        let result = parse(
            "namespace Geom; define Point struct x: float y: float end",
        );
        let frag = extract_fragment("geom.nv", &result);
        assert_eq!(frag.type_declarations.len(), 1);
        let td = &frag.type_declarations[0];
        assert_eq!(td.name, "Geom.Point");
        let fields = match &td.body {
            FragmentTypeBody::Struct { fields } => fields,
            _ => panic!("expected struct"),
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "x");
        assert_eq!(fields[0].type_name, "float");
        assert_eq!(fields[1].name, "y");
        assert_eq!(fields[1].type_name, "float");
    }

    #[test]
    fn fragment_enum_declaration() {
        let result = parse(
            "namespace Color; define RGB enum: int Red = 0 Green = 1 Blue = 2 end",
        );
        let frag = extract_fragment("color.nv", &result);
        assert_eq!(frag.type_declarations.len(), 1);
        let td = &frag.type_declarations[0];
        assert_eq!(td.name, "Color.RGB");
        assert!(matches!(&td.body, FragmentTypeBody::Enum { members, .. } if members.len() == 3));
        if let FragmentTypeBody::Enum { base_type: _, members } = &td.body {
            assert_eq!(members[0], "Red");
        }
    }

    #[test]
    fn fragment_variant_declaration() {
        let result = parse(
            "namespace Expr; define Value variant Int: int Str: str end",
        );
        let frag = extract_fragment("expr.nv", &result);
        assert_eq!(frag.type_declarations.len(), 1);
        assert!(matches!(&frag.type_declarations[0].body, FragmentTypeBody::Variant { cases } if cases.len() == 2));
    }

    #[test]
    fn fragment_trait_declaration() {
        let result = parse(
            "namespace Iter; define Seq trait next(): bool has_next(): bool end",
        );
        let frag = extract_fragment("iter.nv", &result);
        assert_eq!(frag.type_declarations.len(), 1);
        assert!(matches!(&frag.type_declarations[0].body, FragmentTypeBody::Trait { signatures } if signatures.len() == 2));
    }

    #[test]
    fn fragment_function_detection() {
        let result = parse(
            "namespace Math; define add(x: int, y: int): int end",
        );
        let frag = extract_fragment("math.nv", &result);
        assert_eq!(frag.functions.len(), 1);
        assert_eq!(frag.functions[0].name, "Math.add");
    }

    #[test]
    fn fragment_plain_value_skipped() {
        let result = parse("namespace Cfg; define debug true");
        let frag = extract_fragment("cfg.nv", &result);
        assert!(frag.type_declarations.is_empty());
        assert!(frag.functions.is_empty());
    }

    #[test]
    fn fragment_trait_impl() {
        let result = parse(
            "namespace App; implement fmt.Display for MyType end",
        );
        let frag = extract_fragment("app.nv", &result);
        assert_eq!(frag.impl_blocks.len(), 1);
        let ib = &frag.impl_blocks[0];
        assert_eq!(ib.trait_name, Some("fmt.Display".to_string()));
        assert_eq!(ib.target_type, "App.MyType");
    }

    #[test]
    fn fragment_inherent_impl() {
        let result = parse(
            "namespace App; implement for Foo define bar(): unit end define hidden 0 end",
        );
        let frag = extract_fragment("app.nv", &result);
        assert_eq!(frag.impl_blocks.len(), 1);
        let ib = &frag.impl_blocks[0];
        assert!(ib.trait_name.is_none());
        assert_eq!(ib.target_type, "App.Foo");
        assert_eq!(ib.method_bodies.len(), 1);
        assert_eq!(ib.method_bodies[0].name, "App.Foo.bar");
    }

    // ── Assembly tests ────────────────────────────────────────────────────

    #[test]
    fn assemble_single_fragment() {
        let result = parse(
            "namespace Geom; define Point struct x: float y: float end",
        );
        let frag = extract_fragment("geom.nv", &result);
        let bundle = assemble(vec![frag]);

        assert_eq!(bundle.version.major, 1);
        assert_eq!(bundle.files.len(), 1);
        assert_eq!(bundle.files[0], "geom.nv");

        // Type list: "Geom.Point", "float"
        assert_eq!(bundle.type_list.len(), 2);
        assert_eq!(bundle.type_list[0].name, "Geom.Point");
        assert_eq!(bundle.type_list[1].name, "float");

        // Type declarations
        assert_eq!(bundle.type_declarations.len(), 1);
        let td = &bundle.type_declarations[0];
        assert_eq!(td.file_index, 0);
        if let TypeBody::Struct { fields } = &td.body {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].type_index, 1);
        } else {
            panic!("expected struct");
        }

        // No functions
        assert!(bundle.func_list.is_empty());
        assert!(bundle.func_bodies.is_empty());
        assert!(bundle.impl_bodies.is_empty());
    }

    #[test]
    fn assemble_multi_file_dedups_type_list() {
        let r1 = parse("namespace A; define S struct x: int end");
        let r2 = parse("namespace B; define T struct y: int end");
        let f1 = extract_fragment("a.nv", &r1);
        let f2 = extract_fragment("b.nv", &r2);
        let bundle = assemble(vec![f1, f2]);

        assert_eq!(bundle.files.len(), 2);
        // Type list: "A.S", "B.T", "int"
        assert_eq!(bundle.type_list.len(), 3);
        assert_eq!(bundle.type_declarations.len(), 2);
    }

    #[test]
    fn assemble_functions_and_impls() {
        let src = "\
namespace App;
define add(x: int): int end
implement for Foo define bar(): unit end define hidden 0 end
";
        let result = parse(src);
        let frag = extract_fragment("app.nv", &result);
        let bundle = assemble(vec![frag]);

        // Functions: "App.add", "App.Foo.bar" (sorted: 'F' < 'a' → Foo.bar first)
        assert_eq!(bundle.func_list.len(), 2);
        assert_eq!(bundle.func_list[0].name, "App.Foo.bar");
        assert_eq!(bundle.func_list[1].name, "App.add");

        assert_eq!(bundle.func_bodies.len(), 1); // only free functions
        assert_eq!(bundle.impl_bodies.len(), 1);
        assert_eq!(bundle.impl_bodies[0].methods.len(), 1);
    }
}
