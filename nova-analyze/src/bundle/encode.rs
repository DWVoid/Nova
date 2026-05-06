//! Binary encoder for the NVIL bundle format.
//!
//! Produces a byte sequence that can be written to disk or transmitted.
//! All multi-byte integers are **little-endian**.
//!
//! ## File layout
//!
//! ```text
//! [Header 16 bytes]
//!   Magic  : b"NVIL"           (4 bytes)
//!   Major  : u16 LE            (2 bytes)
//!   Minor  : u16 LE            (2 bytes)
//!   Patch  : u16 LE            (2 bytes)
//!   Extra  : u16 LE            (2 bytes)
//!   Resvd  : [0u8; 4]          (4 bytes)
//!
//! [Sections]  (each: Tag(u8) | BodyLen(u32 LE) | Body)
//!   Tag 1  : Metadata
//!   Tag 2  : Dependencies
//!   Tag 3  : FileList
//!   Tag 4  : TypeList
//!   Tag 5  : TypeDeclarations
//!   Tag 6  : ConstList
//!   Tag 7  : ConstDefinitions
//!   Tag 8  : FuncList
//!   Tag 9  : FuncBodies
//!   Tag 10 : ImplBodies
//! ```

use std::io;
use crate::bundle::{
    Bundle, ConstDefinition, ConstListEntry, Dependency, FieldDecl,
    FormatVersion, FuncBody, FuncListEntry, ImplBody, MetadataEntry,
    SemanticBinaryOp, SemanticBlock, SemanticExpr, SemanticFuncBody,
    SemanticStmt, SemanticUnaryOp, SourceSpan, TraitSigDecl,
    TypeBody, TypeDeclaration, TypeListEntry,
};

const TAG_METADATA: u8          = 1;
const TAG_DEPENDENCIES: u8      = 2;
const TAG_FILE_LIST: u8         = 3;
const TAG_TYPE_LIST: u8         = 4;
const TAG_TYPE_DECLS: u8        = 5;
const TAG_CONST_LIST: u8        = 6;
const TAG_CONST_DEFS: u8        = 7;
const TAG_FUNC_LIST: u8         = 8;
const TAG_FUNC_BODIES: u8       = 9;
const TAG_IMPL_BODIES: u8       = 10;

/// Serialize a [`Bundle`] into the NVIL binary format.
pub fn encode_bundle(bundle: &Bundle) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();

    buf.extend_from_slice(b"NVIL");
    buf.extend_from_slice(&bundle.version.major.to_le_bytes());
    buf.extend_from_slice(&bundle.version.minor.to_le_bytes());
    buf.extend_from_slice(&bundle.version.patch.to_le_bytes());
    buf.extend_from_slice(&bundle.version.extra.to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);

    write_section(&mut buf, TAG_METADATA, |w| write_metadata_entries(w, &bundle.metadata))?;
    write_section(&mut buf, TAG_DEPENDENCIES, |w| write_dependencies(w, &bundle.dependencies))?;
    write_section(&mut buf, TAG_FILE_LIST, |w| write_strings(w, &bundle.files))?;
    write_section(&mut buf, TAG_TYPE_LIST, |w| write_type_list(w, &bundle.type_list))?;
    write_section(&mut buf, TAG_TYPE_DECLS, |w| write_type_declarations(w, &bundle.type_declarations))?;
    write_section(&mut buf, TAG_CONST_LIST, |w| write_const_list(w, &bundle.const_list))?;
    write_section(&mut buf, TAG_CONST_DEFS, |w| write_const_definitions(w, &bundle.const_definitions))?;
    write_section(&mut buf, TAG_FUNC_LIST, |w| write_func_list(w, &bundle.func_list))?;
    write_section(&mut buf, TAG_FUNC_BODIES, |w| write_func_bodies(w, &bundle.func_bodies))?;
    write_section(&mut buf, TAG_IMPL_BODIES, |w| write_impl_bodies(w, &bundle.impl_bodies))?;

    Ok(buf)
}

// ---------------------------------------------------------------------------
// Section wrapper
// ---------------------------------------------------------------------------

fn write_section<F>(w: &mut Vec<u8>, tag: u8, f: F) -> io::Result<()>
where F: FnOnce(&mut Vec<u8>) -> io::Result<()> {
    let mut body = Vec::new();
    f(&mut body)?;
    w.push(tag);
    w.extend_from_slice(&(body.len() as u32).to_le_bytes());
    w.extend_from_slice(&body);
    Ok(())
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn write_u8(w: &mut Vec<u8>, v: u8) { w.push(v); }

fn write_u16(w: &mut Vec<u8>, v: u16) { w.extend_from_slice(&v.to_le_bytes()); }

fn write_u32(w: &mut Vec<u8>, v: u32) { w.extend_from_slice(&v.to_le_bytes()); }

fn write_u64(w: &mut Vec<u8>, v: u64) { w.extend_from_slice(&v.to_le_bytes()); }

fn write_string(w: &mut Vec<u8>, s: &str) -> io::Result<()> {
    let bytes = s.as_bytes();
    if bytes.len() > u32::MAX as usize {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "string too long"));
    }
    write_u32(w, bytes.len() as u32);
    w.extend_from_slice(bytes);
    Ok(())
}

fn write_optional_string(w: &mut Vec<u8>, s: &Option<String>) -> io::Result<()> {
    match s {
        Some(v) => { write_u8(w, 1); write_string(w, v)?; }
        None => write_u8(w, 0),
    }
    Ok(())
}

fn write_optional_u32(w: &mut Vec<u8>, v: Option<u32>) {
    match v {
        Some(val) => { write_u8(w, 1); write_u32(w, val); }
        None => write_u8(w, 0),
    }
}

fn write_source_span(w: &mut Vec<u8>, sp: &SourceSpan) {
    write_u32(w, sp.start_byte);
    write_u32(w, sp.end_byte);
    write_u32(w, sp.start_line);
    write_u32(w, sp.start_col);
    write_u32(w, sp.end_line);
    write_u32(w, sp.end_col);
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

fn write_metadata_entries(w: &mut Vec<u8>, entries: &[MetadataEntry]) -> io::Result<()> {
    write_u32(w, entries.len() as u32);
    for entry in entries {
        write_string(w, &entry.key)?;
        write_u32(w, entry.value.len() as u32);
        w.extend_from_slice(&entry.value);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dependencies
// ---------------------------------------------------------------------------

fn write_dependencies(w: &mut Vec<u8>, deps: &[Dependency]) -> io::Result<()> {
    write_u32(w, deps.len() as u32);
    for dep in deps {
        write_string(w, &dep.id)?;
        write_u64(w, dep.anchor_mask);
        write_format_version(w, &dep.version);
        write_metadata_entries(w, &dep.options)?;
    }
    Ok(())
}

fn write_format_version(w: &mut Vec<u8>, v: &FormatVersion) {
    write_u16(w, v.major);
    write_u16(w, v.minor);
    write_u16(w, v.patch);
    write_u16(w, v.extra);
}

// ---------------------------------------------------------------------------
// Strings (for file list)
// ---------------------------------------------------------------------------

fn write_strings(w: &mut Vec<u8>, strings: &[String]) -> io::Result<()> {
    write_u32(w, strings.len() as u32);
    for s in strings {
        write_string(w, s)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Type list
// ---------------------------------------------------------------------------

fn write_type_list(w: &mut Vec<u8>, entries: &[TypeListEntry]) -> io::Result<()> {
    write_u32(w, entries.len() as u32);
    for entry in entries {
        write_u32(w, entry.bundle_id);
        write_string(w, &entry.name)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Type declarations
// ---------------------------------------------------------------------------

fn write_type_declarations(w: &mut Vec<u8>, decls: &[TypeDeclaration]) -> io::Result<()> {
    write_u32(w, decls.len() as u32);
    for decl in decls {
        write_u32(w, decl.type_index);
        write_u32(w, decl.file_index);
        write_source_span(w, &decl.span);
        write_type_body(w, &decl.body)?;
    }
    Ok(())
}

fn write_type_body(w: &mut Vec<u8>, body: &TypeBody) -> io::Result<()> {
    match body {
        TypeBody::Struct { fields } => {
            write_u8(w, 0);
            write_u32(w, fields.len() as u32);
            for f in fields {
                write_field_decl(w, f)?;
            }
        }
        TypeBody::Enum { base_type, members } => {
            write_u8(w, 1);
            write_u32(w, *base_type);
            write_u32(w, members.len() as u32);
            for m in members {
                write_string(w, m)?;
            }
        }
        TypeBody::Variant { cases } => {
            write_u8(w, 2);
            write_u32(w, cases.len() as u32);
            for c in cases {
                write_string(w, &c.name)?;
                write_u32(w, c.type_index);
                write_source_span(w, &c.span);
            }
        }
        TypeBody::Trait { signatures } => {
            write_u8(w, 3);
            write_u32(w, signatures.len() as u32);
            for s in signatures {
                write_trait_sig_decl(w, s)?;
            }
        }
    }
    Ok(())
}

fn write_field_decl(w: &mut Vec<u8>, f: &FieldDecl) -> io::Result<()> {
    write_string(w, &f.name)?;
    write_u32(w, f.type_index);
    write_source_span(w, &f.span);
    Ok(())
}

fn write_trait_sig_decl(w: &mut Vec<u8>, s: &TraitSigDecl) -> io::Result<()> {
    write_string(w, &s.name)?;
    write_u32(w, s.params.len() as u32);
    for p in &s.params {
        write_string(w, &p.name)?;
        write_optional_u32(w, p.type_index);
    }
    write_u32(w, s.return_type);
    write_source_span(w, &s.span);
    Ok(())
}

// ---------------------------------------------------------------------------
// Constant list
// ---------------------------------------------------------------------------

fn write_const_list(w: &mut Vec<u8>, entries: &[ConstListEntry]) -> io::Result<()> {
    write_u32(w, entries.len() as u32);
    for entry in entries {
        write_u32(w, entry.bundle_id);
        write_string(w, &entry.name)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Constant definitions
// ---------------------------------------------------------------------------

fn write_const_definitions(w: &mut Vec<u8>, defs: &[ConstDefinition]) -> io::Result<()> {
    write_u32(w, defs.len() as u32);
    for def in defs {
        write_u32(w, def.const_index);
        write_u32(w, def.file_index);
        write_source_span(w, &def.span);
        write_optional_u32(w, def.type_index);
        write_semantic_expr(w, &def.expr)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Function list
// ---------------------------------------------------------------------------

fn write_func_list(w: &mut Vec<u8>, entries: &[FuncListEntry]) -> io::Result<()> {
    write_u32(w, entries.len() as u32);
    for entry in entries {
        write_u32(w, entry.bundle_id);
        write_string(w, &entry.name)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Function bodies
// ---------------------------------------------------------------------------

fn write_func_bodies(w: &mut Vec<u8>, bodies: &[FuncBody]) -> io::Result<()> {
    write_u32(w, bodies.len() as u32);
    for body in bodies {
        write_u32(w, body.func_index);
        write_u32(w, body.file_index);
        write_source_span(w, &body.span);
        write_semantic_func_body(w, &body.semantic)?;
    }
    Ok(())
}

fn write_semantic_func_body(w: &mut Vec<u8>, s: &SemanticFuncBody) -> io::Result<()> {
    write_u32(w, s.params.len() as u32);
    for p in &s.params {
        write_string(w, &p.name)?;
        write_optional_u32(w, p.type_index);
    }
    write_optional_u32(w, s.return_type);
    write_u32(w, s.locals.len() as u32);
    for l in &s.locals {
        write_string(w, &l.name)?;
        write_optional_u32(w, l.type_index);
    }
    write_semantic_block(w, &s.body)?;
    Ok(())
}

fn write_semantic_block(w: &mut Vec<u8>, b: &SemanticBlock) -> io::Result<()> {
    write_u32(w, b.locals.len() as u32);
    for l in &b.locals {
        write_string(w, &l.name)?;
        write_optional_u32(w, l.type_index);
    }
    write_u32(w, b.stmts.len() as u32);
    for s in &b.stmts {
        write_semantic_stmt(w, s)?;
    }
    Ok(())
}

fn write_semantic_stmt(w: &mut Vec<u8>, stmt: &SemanticStmt) -> io::Result<()> {
    match stmt {
        SemanticStmt::Empty => write_u8(w, 0),
        SemanticStmt::VarDecl { name, type_index, init, is_var } => {
            write_u8(w, 1);
            write_string(w, name)?;
            write_optional_u32(w, *type_index);
            match init {
                Some(e) => { write_u8(w, 1); write_semantic_expr(w, e)?; }
                None => write_u8(w, 0),
            }
            write_u8(w, if *is_var { 1 } else { 0 });
        }
        SemanticStmt::Assign { targets, value } => {
            write_u8(w, 2);
            write_u32(w, targets.len() as u32);
            for t in targets { write_semantic_expr(w, t)?; }
            write_semantic_expr(w, value)?;
        }
        SemanticStmt::Call { expr } => {
            write_u8(w, 3);
            write_semantic_expr(w, expr)?;
        }
        SemanticStmt::Do(block) => {
            write_u8(w, 4);
            write_semantic_block(w, block)?;
        }
        SemanticStmt::While { cond, body } => {
            write_u8(w, 5);
            write_semantic_expr(w, cond)?;
            write_semantic_block(w, body)?;
        }
        SemanticStmt::Repeat { body, until } => {
            write_u8(w, 6);
            write_semantic_block(w, body)?;
            write_semantic_expr(w, until)?;
        }
        SemanticStmt::If { cond, then, else_ifs, else_block } => {
            write_u8(w, 7);
            write_semantic_expr(w, cond)?;
            write_semantic_block(w, then)?;
            write_u32(w, else_ifs.len() as u32);
            for (e_cond, e_then) in else_ifs {
                write_semantic_expr(w, e_cond)?;
                write_semantic_block(w, e_then)?;
            }
            match else_block {
                Some(b) => { write_u8(w, 1); write_semantic_block(w, b)?; }
                None => write_u8(w, 0),
            }
        }
        SemanticStmt::ForNumeric { var, start, end, step, body } => {
            write_u8(w, 8);
            write_string(w, var)?;
            write_semantic_expr(w, start)?;
            write_semantic_expr(w, end)?;
            match step {
                Some(e) => { write_u8(w, 1); write_semantic_expr(w, e)?; }
                None => write_u8(w, 0),
            }
            write_semantic_block(w, body)?;
        }
        SemanticStmt::ForGeneric { vars, iter, body } => {
            write_u8(w, 9);
            write_u32(w, vars.len() as u32);
            for v in vars { write_string(w, v)?; }
            write_semantic_expr(w, iter)?;
            write_semantic_block(w, body)?;
        }
        SemanticStmt::Break => write_u8(w, 10),
        SemanticStmt::Continue => write_u8(w, 11),
        SemanticStmt::Return { values } => {
            write_u8(w, 12);
            write_u32(w, values.len() as u32);
            for v in values { write_semantic_expr(w, v)?; }
        }
        SemanticStmt::Label(s) => { write_u8(w, 13); write_string(w, s)?; }
        SemanticStmt::Goto(s) => { write_u8(w, 14); write_string(w, s)?; }
    }
    Ok(())
}

fn write_semantic_expr(w: &mut Vec<u8>, expr: &SemanticExpr) -> io::Result<()> {
    match expr {
        SemanticExpr::Nil => write_u8(w, 0),
        SemanticExpr::Bool(b) => { write_u8(w, 1); write_u8(w, if *b { 1 } else { 0 }); }
        SemanticExpr::Number(s) => { write_u8(w, 2); write_string(w, s)?; }
        SemanticExpr::String(s) => { write_u8(w, 3); write_string(w, s)?; }
        SemanticExpr::Name(s) => { write_u8(w, 4); write_string(w, s)?; }
        SemanticExpr::Field { object, field } => {
            write_u8(w, 5);
            write_semantic_expr(w, object)?;
            write_string(w, field)?;
        }
        SemanticExpr::Index { object, index } => {
            write_u8(w, 6);
            write_semantic_expr(w, object)?;
            write_semantic_expr(w, index)?;
        }
        SemanticExpr::Call { func, args } => {
            write_u8(w, 7);
            write_semantic_expr(w, func)?;
            write_u32(w, args.len() as u32);
            for a in args { write_semantic_expr(w, a)?; }
        }
        SemanticExpr::Unary { op, expr } => {
            write_u8(w, 8);
            write_semantic_unary_op(w, op);
            write_semantic_expr(w, expr)?;
        }
        SemanticExpr::Binary { op, left, right } => {
            write_u8(w, 9);
            write_semantic_binary_op(w, op);
            write_semantic_expr(w, left)?;
            write_semantic_expr(w, right)?;
        }
        SemanticExpr::Lambda { params, return_type, body } => {
            write_u8(w, 10);
            write_u32(w, params.len() as u32);
            for p in params {
                write_string(w, &p.name)?;
                write_optional_u32(w, p.type_index);
            }
            write_optional_u32(w, *return_type);
            write_semantic_block(w, body)?;
        }
        SemanticExpr::VarDecl { name, type_index, init, is_var } => {
            write_u8(w, 11);
            write_string(w, name)?;
            write_optional_u32(w, *type_index);
            match init {
                Some(e) => { write_u8(w, 1); write_semantic_expr(w, e)?; }
                None => write_u8(w, 0),
            }
            write_u8(w, if *is_var { 1 } else { 0 });
        }
    }
    Ok(())
}

fn write_semantic_unary_op(w: &mut Vec<u8>, op: &SemanticUnaryOp) {
    match op {
        SemanticUnaryOp::Neg => write_u8(w, 0),
        SemanticUnaryOp::Not => write_u8(w, 1),
        SemanticUnaryOp::Len => write_u8(w, 2),
        SemanticUnaryOp::BitNot => write_u8(w, 3),
    }
}

fn write_semantic_binary_op(w: &mut Vec<u8>, op: &SemanticBinaryOp) {
    match op {
        SemanticBinaryOp::Add => write_u8(w, 0),
        SemanticBinaryOp::Sub => write_u8(w, 1),
        SemanticBinaryOp::Mul => write_u8(w, 2),
        SemanticBinaryOp::Div => write_u8(w, 3),
        SemanticBinaryOp::FloorDiv => write_u8(w, 4),
        SemanticBinaryOp::Mod => write_u8(w, 5),
        SemanticBinaryOp::Pow => write_u8(w, 6),
        SemanticBinaryOp::Eq => write_u8(w, 7),
        SemanticBinaryOp::NotEq => write_u8(w, 8),
        SemanticBinaryOp::Less => write_u8(w, 9),
        SemanticBinaryOp::LessEq => write_u8(w, 10),
        SemanticBinaryOp::Greater => write_u8(w, 11),
        SemanticBinaryOp::GreaterEq => write_u8(w, 12),
        SemanticBinaryOp::And => write_u8(w, 13),
        SemanticBinaryOp::Or => write_u8(w, 14),
        SemanticBinaryOp::Concat => write_u8(w, 15),
        SemanticBinaryOp::BitAnd => write_u8(w, 16),
        SemanticBinaryOp::BitOr => write_u8(w, 17),
        SemanticBinaryOp::BitXor => write_u8(w, 18),
        SemanticBinaryOp::Shl => write_u8(w, 19),
        SemanticBinaryOp::Shr => write_u8(w, 20),
    }
}

// ---------------------------------------------------------------------------
// Impl bodies
// ---------------------------------------------------------------------------

fn write_impl_bodies(w: &mut Vec<u8>, bodies: &[ImplBody]) -> io::Result<()> {
    write_u32(w, bodies.len() as u32);
    for body in bodies {
        write_u32(w, body.type_index);
        write_optional_u32(w, body.trait_index);
        write_u32(w, body.file_index);
        write_source_span(w, &body.span);

        write_u32(w, body.methods.len() as u32);
        for m in &body.methods {
            write_u32(w, m.func_index);
            write_u32(w, m.file_index);
            write_source_span(w, &m.span);
            write_semantic_func_body(w, &m.semantic)?;
        }
    }
    Ok(())
}
