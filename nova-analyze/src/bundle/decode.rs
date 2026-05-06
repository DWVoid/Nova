//! Binary decoder for the NVIL bundle format.
//!
//! Reads the byte sequence produced by [`encode::encode_bundle`] and returns a
//! [`Bundle`].  Unknown sections are silently skipped for forward compatibility.

use std::io::{Read, Cursor};
use crate::bundle::{
    Bundle, ConstDefinition, ConstListEntry, Dependency, FieldDecl,
    FormatVersion, FuncBody, FuncListEntry, ImplBody, MetadataEntry, Param,
    SemanticBinaryOp, SemanticBlock, SemanticExpr, SemanticFuncBody, SemanticLocal,
    SemanticParam, SemanticStmt, SemanticUnaryOp, SourceSpan, TraitSigDecl,
    TypeBody, TypeDeclaration, TypeListEntry, VariantCaseDecl,
};

/// Deserialize a [`Bundle`] from NVIL binary format bytes.
pub fn decode_bundle(data: &[u8]) -> Result<Bundle, String> {
    let mut c = Cursor::new(data);

    let magic = read_bytes(&mut c, 4)?;
    if &magic != b"NVIL" {
        return Err(format!("invalid magic: expected NVIL, got {:?}", magic));
    }

    let major = read_u16(&mut c)?;
    let minor = read_u16(&mut c)?;
    let patch = read_u16(&mut c)?;
    let extra = read_u16(&mut c)?;
    let _reserved = read_bytes(&mut c, 4)?;

    let version = FormatVersion { major, minor, patch, extra };

    let mut metadata          = Vec::new();
    let mut dependencies      = Vec::new();
    let mut files             = Vec::new();
    let mut type_list         = Vec::new();
    let mut type_decls        = Vec::new();
    let mut const_list        = Vec::new();
    let mut const_defs        = Vec::new();
    let mut func_list         = Vec::new();
    let mut func_bodies       = Vec::new();
    let mut impl_bodies       = Vec::new();

    loop {
        let tag = match read_u8_opt(&mut c) {
            Some(t) => t,
            None => break,
        };
        let body_len = read_u32(&mut c)? as usize;
        let body = read_bytes(&mut c, body_len)?;
        let mut bc = Cursor::new(body.as_slice());

        match tag {
            1 => metadata = read_metadata_entries(&mut bc)?,
            2 => dependencies = read_dependencies(&mut bc)?,
            3 => files = read_strings(&mut bc)?,
            4 => type_list = read_type_list(&mut bc)?,
            5 => type_decls = read_type_declarations(&mut bc)?,
            6 => const_list = read_const_list(&mut bc)?,
            7 => const_defs = read_const_definitions(&mut bc)?,
            8 => func_list = read_func_list(&mut bc)?,
            9 => func_bodies = read_func_bodies(&mut bc)?,
            10 => impl_bodies = read_impl_bodies(&mut bc)?,
            _ => { /* skip unknown section */ }
        }
    }

    Ok(Bundle {
        version,
        metadata,
        dependencies,
        files,
        type_list,
        type_declarations: type_decls,
        const_list,
        const_definitions: const_defs,
        func_list,
        func_bodies,
        impl_bodies,
    })
}

// ---------------------------------------------------------------------------
// Primitive readers
// ---------------------------------------------------------------------------

fn read_exact(c: &mut Cursor<&[u8]>, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    c.read_exact(&mut buf).map_err(|e| format!("truncated input: {}", e))?;
    Ok(buf)
}

fn read_bytes(c: &mut Cursor<&[u8]>, n: usize) -> Result<Vec<u8>, String> {
    read_exact(c, n)
}

fn read_u8(c: &mut Cursor<&[u8]>) -> Result<u8, String> {
    let b = read_exact(c, 1)?;
    Ok(b[0])
}

fn read_u8_opt(c: &mut Cursor<&[u8]>) -> Option<u8> {
    let mut buf = [0u8; 1];
    c.read_exact(&mut buf).ok()?;
    Some(buf[0])
}

fn read_u16(c: &mut Cursor<&[u8]>) -> Result<u16, String> {
    let b = read_exact(c, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(c: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let b = read_exact(c, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_u64(c: &mut Cursor<&[u8]>) -> Result<u64, String> {
    let b = read_exact(c, 8)?;
    Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn read_string(c: &mut Cursor<&[u8]>) -> Result<String, String> {
    let len = read_u32(c)? as usize;
    let bytes = read_exact(c, len)?;
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {}", e))
}

fn read_optional_string(c: &mut Cursor<&[u8]>) -> Result<Option<String>, String> {
    let tag = read_u8(c)?;
    if tag == 0 { Ok(None) } else { Ok(Some(read_string(c)?)) }
}

fn read_optional_u32(c: &mut Cursor<&[u8]>) -> Result<Option<u32>, String> {
    let tag = read_u8(c)?;
    if tag == 0 { Ok(None) } else { Ok(Some(read_u32(c)?)) }
}

fn read_source_span(c: &mut Cursor<&[u8]>) -> Result<SourceSpan, String> {
    Ok(SourceSpan {
        start_byte: read_u32(c)?,
        end_byte: read_u32(c)?,
        start_line: read_u32(c)?,
        start_col: read_u32(c)?,
        end_line: read_u32(c)?,
        end_col: read_u32(c)?,
    })
}

// ---------------------------------------------------------------------------
// Metadata readers
// ---------------------------------------------------------------------------

fn read_metadata_entries(c: &mut Cursor<&[u8]>) -> Result<Vec<MetadataEntry>, String> {
    let count = read_u32(c)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = read_string(c)?;
        let val_len = read_u32(c)? as usize;
        let value = read_exact(c, val_len)?;
        entries.push(MetadataEntry { key, value });
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Dependency readers
// ---------------------------------------------------------------------------

fn read_dependencies(c: &mut Cursor<&[u8]>) -> Result<Vec<Dependency>, String> {
    let count = read_u32(c)? as usize;
    let mut deps = Vec::with_capacity(count);
    for _ in 0..count {
        let id = read_string(c)?;
        let anchor_mask = read_u64(c)?;
        let version = read_format_version(c)?;
        let options = read_metadata_entries(c)?;
        deps.push(Dependency { id, anchor_mask, version, options });
    }
    Ok(deps)
}

fn read_format_version(c: &mut Cursor<&[u8]>) -> Result<FormatVersion, String> {
    Ok(FormatVersion {
        major: read_u16(c)?,
        minor: read_u16(c)?,
        patch: read_u16(c)?,
        extra: read_u16(c)?,
    })
}

// ---------------------------------------------------------------------------
// String list reader
// ---------------------------------------------------------------------------

fn read_strings(c: &mut Cursor<&[u8]>) -> Result<Vec<String>, String> {
    let count = read_u32(c)? as usize;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        list.push(read_string(c)?);
    }
    Ok(list)
}

// ---------------------------------------------------------------------------
// Type list readers
// ---------------------------------------------------------------------------

fn read_type_list(c: &mut Cursor<&[u8]>) -> Result<Vec<TypeListEntry>, String> {
    let count = read_u32(c)? as usize;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        let bundle_id = read_u32(c)?;
        let name = read_string(c)?;
        list.push(TypeListEntry { bundle_id, name });
    }
    Ok(list)
}

// ---------------------------------------------------------------------------
// Type declaration readers
// ---------------------------------------------------------------------------

fn read_type_declarations(c: &mut Cursor<&[u8]>) -> Result<Vec<TypeDeclaration>, String> {
    let count = read_u32(c)? as usize;
    let mut decls = Vec::with_capacity(count);
    for _ in 0..count {
        let type_index = read_u32(c)?;
        let file_index = read_u32(c)?;
        let span = read_source_span(c)?;
        let body = read_type_body(c)?;
        decls.push(TypeDeclaration { type_index, file_index, span, body });
    }
    Ok(decls)
}

fn read_type_body(c: &mut Cursor<&[u8]>) -> Result<TypeBody, String> {
    let tag = read_u8(c)?;
    match tag {
        0 => {
            let field_count = read_u32(c)? as usize;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let name = read_string(c)?;
                let type_index = read_u32(c)?;
                let span = read_source_span(c)?;
                fields.push(FieldDecl { name, type_index, span });
            }
            Ok(TypeBody::Struct { fields })
        }
        1 => {
            let base_type = read_u32(c)?;
            let member_count = read_u32(c)? as usize;
            let mut members = Vec::with_capacity(member_count);
            for _ in 0..member_count {
                members.push(read_string(c)?);
            }
            Ok(TypeBody::Enum { base_type, members })
        }
        2 => {
            let case_count = read_u32(c)? as usize;
            let mut cases = Vec::with_capacity(case_count);
            for _ in 0..case_count {
                let name = read_string(c)?;
                let type_index = read_u32(c)?;
                let span = read_source_span(c)?;
                cases.push(VariantCaseDecl { name, type_index, span });
            }
            Ok(TypeBody::Variant { cases })
        }
        3 => {
            let sig_count = read_u32(c)? as usize;
            let mut signatures = Vec::with_capacity(sig_count);
            for _ in 0..sig_count {
                let name = read_string(c)?;
                let param_count = read_u32(c)? as usize;
                let mut params = Vec::with_capacity(param_count);
                for _ in 0..param_count {
                    let p_name = read_string(c)?;
                    let type_index = read_optional_u32(c)?;
                    params.push(Param { name: p_name, type_index });
                }
                let return_type = read_u32(c)?;
                let span = read_source_span(c)?;
                signatures.push(TraitSigDecl { name, params, return_type, span });
            }
            Ok(TypeBody::Trait { signatures })
        }
        other => Err(format!("unknown TypeBody tag: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// Constant list readers
// ---------------------------------------------------------------------------

fn read_const_list(c: &mut Cursor<&[u8]>) -> Result<Vec<ConstListEntry>, String> {
    let count = read_u32(c)? as usize;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        let bundle_id = read_u32(c)?;
        let name = read_string(c)?;
        list.push(ConstListEntry { bundle_id, name });
    }
    Ok(list)
}

// ---------------------------------------------------------------------------
// Constant definition readers
// ---------------------------------------------------------------------------

fn read_const_definitions(c: &mut Cursor<&[u8]>) -> Result<Vec<ConstDefinition>, String> {
    let count = read_u32(c)? as usize;
    let mut defs = Vec::with_capacity(count);
    for _ in 0..count {
        let const_index = read_u32(c)?;
        let file_index = read_u32(c)?;
        let span = read_source_span(c)?;
        let type_index = read_optional_u32(c)?;
        let expr = read_semantic_expr(c)?;
        defs.push(ConstDefinition { const_index, file_index, span, type_index, expr });
    }
    Ok(defs)
}

// ---------------------------------------------------------------------------
// Function list reader
// ---------------------------------------------------------------------------

fn read_func_list(c: &mut Cursor<&[u8]>) -> Result<Vec<FuncListEntry>, String> {
    let count = read_u32(c)? as usize;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        let bundle_id = read_u32(c)?;
        let name = read_string(c)?;
        list.push(FuncListEntry { bundle_id, name });
    }
    Ok(list)
}

// ---------------------------------------------------------------------------
// Function body reader
// ---------------------------------------------------------------------------

fn read_func_bodies(c: &mut Cursor<&[u8]>) -> Result<Vec<FuncBody>, String> {
    let count = read_u32(c)? as usize;
    let mut bodies = Vec::with_capacity(count);
    for _ in 0..count {
        let func_index = read_u32(c)?;
        let file_index = read_u32(c)?;
        let span = read_source_span(c)?;
        let semantic = read_semantic_func_body(c)?;
        bodies.push(FuncBody { func_index, file_index, span, semantic });
    }
    Ok(bodies)
}

fn read_semantic_func_body(c: &mut Cursor<&[u8]>) -> Result<SemanticFuncBody, String> {
    let param_count = read_u32(c)? as usize;
    let mut params = Vec::with_capacity(param_count);
    for _ in 0..param_count {
        let name = read_string(c)?;
        let type_index = read_optional_u32(c)?;
        params.push(SemanticParam { name, type_index });
    }
    let return_type = read_optional_u32(c)?;
    let local_count = read_u32(c)? as usize;
    let mut locals = Vec::with_capacity(local_count);
    for _ in 0..local_count {
        let name = read_string(c)?;
        let type_index = read_optional_u32(c)?;
        locals.push(SemanticLocal { name, type_index });
    }
    let body = read_semantic_block(c)?;
    Ok(SemanticFuncBody { params, return_type, locals, body })
}

fn read_semantic_block(c: &mut Cursor<&[u8]>) -> Result<SemanticBlock, String> {
    let local_count = read_u32(c)? as usize;
    let mut locals = Vec::with_capacity(local_count);
    for _ in 0..local_count {
        let name = read_string(c)?;
        let type_index = read_optional_u32(c)?;
        locals.push(SemanticLocal { name, type_index });
    }
    let stmt_count = read_u32(c)? as usize;
    let mut stmts = Vec::with_capacity(stmt_count);
    for _ in 0..stmt_count {
        stmts.push(read_semantic_stmt(c)?);
    }
    Ok(SemanticBlock { locals, stmts })
}

fn read_semantic_stmt(c: &mut Cursor<&[u8]>) -> Result<SemanticStmt, String> {
    let tag = read_u8(c)?;
    match tag {
        0 => Ok(SemanticStmt::Empty),
        1 => {
            let name = read_string(c)?;
            let type_index = read_optional_u32(c)?;
            let init = match read_u8(c)? {
                0 => None,
                _ => Some(Box::new(read_semantic_expr(c)?)),
            };
            let is_var = read_u8(c)? != 0;
            Ok(SemanticStmt::VarDecl { name, type_index, init, is_var })
        }
        2 => {
            let target_count = read_u32(c)? as usize;
            let mut targets = Vec::with_capacity(target_count);
            for _ in 0..target_count { targets.push(read_semantic_expr(c)?); }
            let value = read_semantic_expr(c)?;
            Ok(SemanticStmt::Assign { targets, value })
        }
        3 => {
            let expr = read_semantic_expr(c)?;
            Ok(SemanticStmt::Call { expr })
        }
        4 => {
            let block = read_semantic_block(c)?;
            Ok(SemanticStmt::Do(block))
        }
        5 => {
            let cond = read_semantic_expr(c)?;
            let body = read_semantic_block(c)?;
            Ok(SemanticStmt::While { cond, body })
        }
        6 => {
            let body = read_semantic_block(c)?;
            let until = read_semantic_expr(c)?;
            Ok(SemanticStmt::Repeat { body, until })
        }
        7 => {
            let cond = read_semantic_expr(c)?;
            let then = read_semantic_block(c)?;
            let else_if_count = read_u32(c)? as usize;
            let mut else_ifs = Vec::with_capacity(else_if_count);
            for _ in 0..else_if_count {
                let e_cond = read_semantic_expr(c)?;
                let e_then = read_semantic_block(c)?;
                else_ifs.push((e_cond, e_then));
            }
            let else_block = match read_u8(c)? {
                0 => None,
                _ => Some(read_semantic_block(c)?),
            };
            Ok(SemanticStmt::If { cond, then, else_ifs, else_block })
        }
        8 => {
            let var = read_string(c)?;
            let start = read_semantic_expr(c)?;
            let end = read_semantic_expr(c)?;
            let step = match read_u8(c)? {
                0 => None,
                _ => Some(read_semantic_expr(c)?),
            };
            let body = read_semantic_block(c)?;
            Ok(SemanticStmt::ForNumeric { var, start, end, step, body })
        }
        9 => {
            let var_count = read_u32(c)? as usize;
            let mut vars = Vec::with_capacity(var_count);
            for _ in 0..var_count { vars.push(read_string(c)?); }
            let iter = read_semantic_expr(c)?;
            let body = read_semantic_block(c)?;
            Ok(SemanticStmt::ForGeneric { vars, iter, body })
        }
        10 => Ok(SemanticStmt::Break),
        11 => Ok(SemanticStmt::Continue),
        12 => {
            let value_count = read_u32(c)? as usize;
            let mut values = Vec::with_capacity(value_count);
            for _ in 0..value_count { values.push(read_semantic_expr(c)?); }
            Ok(SemanticStmt::Return { values })
        }
        13 => Ok(SemanticStmt::Label(read_string(c)?)),
        14 => Ok(SemanticStmt::Goto(read_string(c)?)),
        other => Err(format!("unknown SemanticStmt tag: {}", other)),
    }
}

fn read_semantic_expr(c: &mut Cursor<&[u8]>) -> Result<SemanticExpr, String> {
    let tag = read_u8(c)?;
    match tag {
        0 => Ok(SemanticExpr::Nil),
        1 => Ok(SemanticExpr::Bool(read_u8(c)? != 0)),
        2 => Ok(SemanticExpr::Number(read_string(c)?)),
        3 => Ok(SemanticExpr::String(read_string(c)?)),
        4 => Ok(SemanticExpr::Name(read_string(c)?)),
        5 => {
            let object = Box::new(read_semantic_expr(c)?);
            let field = read_string(c)?;
            Ok(SemanticExpr::Field { object, field })
        }
        6 => {
            let object = Box::new(read_semantic_expr(c)?);
            let index = Box::new(read_semantic_expr(c)?);
            Ok(SemanticExpr::Index { object, index })
        }
        7 => {
            let func = Box::new(read_semantic_expr(c)?);
            let arg_count = read_u32(c)? as usize;
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count { args.push(read_semantic_expr(c)?); }
            Ok(SemanticExpr::Call { func, args })
        }
        8 => {
            let op = read_semantic_unary_op(c)?;
            let expr = Box::new(read_semantic_expr(c)?);
            Ok(SemanticExpr::Unary { op, expr })
        }
        9 => {
            let op = read_semantic_binary_op(c)?;
            let left = Box::new(read_semantic_expr(c)?);
            let right = Box::new(read_semantic_expr(c)?);
            Ok(SemanticExpr::Binary { op, left, right })
        }
        10 => {
            let param_count = read_u32(c)? as usize;
            let mut params = Vec::with_capacity(param_count);
            for _ in 0..param_count {
                let name = read_string(c)?;
                let type_index = read_optional_u32(c)?;
                params.push(SemanticParam { name, type_index });
            }
            let return_type = read_optional_u32(c)?;
            let body = read_semantic_block(c)?;
            Ok(SemanticExpr::Lambda { params, return_type, body })
        }
        11 => {
            let name = read_string(c)?;
            let type_index = read_optional_u32(c)?;
            let init = match read_u8(c)? {
                0 => None,
                _ => Some(Box::new(read_semantic_expr(c)?)),
            };
            let is_var = read_u8(c)? != 0;
            Ok(SemanticExpr::VarDecl { name, type_index, init, is_var })
        }
        other => Err(format!("unknown SemanticExpr tag: {}", other)),
    }
}

fn read_semantic_unary_op(c: &mut Cursor<&[u8]>) -> Result<SemanticUnaryOp, String> {
    match read_u8(c)? {
        0 => Ok(SemanticUnaryOp::Neg),
        1 => Ok(SemanticUnaryOp::Not),
        2 => Ok(SemanticUnaryOp::Len),
        3 => Ok(SemanticUnaryOp::BitNot),
        other => Err(format!("unknown SemanticUnaryOp tag: {}", other)),
    }
}

fn read_semantic_binary_op(c: &mut Cursor<&[u8]>) -> Result<SemanticBinaryOp, String> {
    match read_u8(c)? {
        0 => Ok(SemanticBinaryOp::Add),
        1 => Ok(SemanticBinaryOp::Sub),
        2 => Ok(SemanticBinaryOp::Mul),
        3 => Ok(SemanticBinaryOp::Div),
        4 => Ok(SemanticBinaryOp::FloorDiv),
        5 => Ok(SemanticBinaryOp::Mod),
        6 => Ok(SemanticBinaryOp::Pow),
        7 => Ok(SemanticBinaryOp::Eq),
        8 => Ok(SemanticBinaryOp::NotEq),
        9 => Ok(SemanticBinaryOp::Less),
        10 => Ok(SemanticBinaryOp::LessEq),
        11 => Ok(SemanticBinaryOp::Greater),
        12 => Ok(SemanticBinaryOp::GreaterEq),
        13 => Ok(SemanticBinaryOp::And),
        14 => Ok(SemanticBinaryOp::Or),
        15 => Ok(SemanticBinaryOp::Concat),
        16 => Ok(SemanticBinaryOp::BitAnd),
        17 => Ok(SemanticBinaryOp::BitOr),
        18 => Ok(SemanticBinaryOp::BitXor),
        19 => Ok(SemanticBinaryOp::Shl),
        20 => Ok(SemanticBinaryOp::Shr),
        other => Err(format!("unknown SemanticBinaryOp tag: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// Impl body reader
// ---------------------------------------------------------------------------

fn read_impl_bodies(c: &mut Cursor<&[u8]>) -> Result<Vec<ImplBody>, String> {
    let count = read_u32(c)? as usize;
    let mut bodies = Vec::with_capacity(count);
    for _ in 0..count {
        let type_index = read_u32(c)?;
        let trait_index = read_optional_u32(c)?;
        let file_index = read_u32(c)?;
        let span = read_source_span(c)?;
        let method_count = read_u32(c)? as usize;
        let mut methods = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            let func_index = read_u32(c)?;
            let file_index = read_u32(c)?;
            let span = read_source_span(c)?;
            let semantic = read_semantic_func_body(c)?;
            methods.push(FuncBody { func_index, file_index, span, semantic });
        }
        bodies.push(ImplBody { type_index, trait_index, file_index, span, methods });
    }
    Ok(bodies)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{encode, assemble, extract_fragment, MetadataEntry};

    fn parse(src: &str) -> crate::syntax::SyntaxResult {
        let lex = crate::lexical::transform(src).unwrap();
        crate::syntax::transform(lex).unwrap()
    }

    #[test]
    fn round_trip_empty_bundle() {
        let bundle = Bundle {
            version: FormatVersion::V1,
            metadata: vec![],
            dependencies: vec![],
            files: vec!["main.nv".to_string()],
            type_list: vec![],
            type_declarations: vec![],
            const_list: vec![],
            const_definitions: vec![],
            func_list: vec![],
            func_bodies: vec![],
            impl_bodies: vec![],
        };
        let bytes = encode::encode_bundle(&bundle).unwrap();
        let decoded = decode_bundle(&bytes).unwrap();
        assert_eq!(decoded.version.major, 1);
        assert_eq!(decoded.files, vec!["main.nv"]);
        assert!(decoded.type_list.is_empty());
    }

    #[test]
    fn round_trip_with_data() {
        let bundle = Bundle {
            version: FormatVersion::V1,
            metadata: vec![
                MetadataEntry { key: "com.nova.bundle.name".into(), value: b"test".to_vec() },
            ],
            dependencies: vec![],
            files: vec!["a.nv".to_string(), "b.nv".to_string()],
            type_list: vec![
                TypeListEntry { bundle_id: 0, name: "A.Foo".into() },
                TypeListEntry { bundle_id: 0, name: "int".into() },
            ],
            type_declarations: vec![
                TypeDeclaration {
                    type_index: 0,
                    file_index: 0,
                    span: SourceSpan { start_byte: 0, end_byte: 10, start_line: 1, start_col: 0, end_line: 1, end_col: 10 },
                    body: TypeBody::Struct {
                        fields: vec![
                            FieldDecl { name: "x".into(), type_index: 1, span: SourceSpan { start_byte: 5, end_byte: 6, start_line: 1, start_col: 5, end_line: 1, end_col: 6 } },
                        ],
                    },
                },
            ],
            const_list: vec![],
            const_definitions: vec![],
            func_list: vec![
                FuncListEntry { bundle_id: 0, name: "A.add".into() },
            ],
            func_bodies: vec![
                FuncBody {
                    func_index: 0,
                    file_index: 0,
                    span: SourceSpan { start_byte: 0, end_byte: 20, start_line: 1, start_col: 0, end_line: 1, end_col: 20 },
                    semantic: SemanticFuncBody {
                        params: vec![],
                        return_type: None,
                        locals: vec![],
                        body: SemanticBlock { locals: vec![], stmts: vec![] },
                    },
                },
            ],
            impl_bodies: vec![],
        };
        let bytes = encode::encode_bundle(&bundle).unwrap();
        let decoded = decode_bundle(&bytes).unwrap();
        assert_eq!(decoded.metadata.len(), 1);
        assert_eq!(decoded.metadata[0].key, "com.nova.bundle.name");
        assert_eq!(decoded.files.len(), 2);
        assert_eq!(decoded.type_list.len(), 2);
        assert_eq!(decoded.type_declarations.len(), 1);
        assert_eq!(decoded.func_list.len(), 1);
        assert_eq!(decoded.func_bodies.len(), 1);
        assert!(matches!(&decoded.type_declarations[0].body, TypeBody::Struct { fields } if fields.len() == 1));
    }

    #[test]
    fn round_trip_from_pipeline() {
        let src = "\
namespace App;
define Point struct x: int y: int end
define add(x: int): int end
implement for Foo define bar(): unit end end
";
        let result = parse(src);
        let frag = extract_fragment("app.nv", &result);
        let bundle = assemble(vec![frag]);

        let bytes = encode::encode_bundle(&bundle).unwrap();
        let decoded = decode_bundle(&bytes).unwrap();

        assert_eq!(decoded.files, vec!["app.nv"]);
        assert_eq!(decoded.type_list.len(), 4); // App.Point, int, App.Foo, unit
        assert_eq!(decoded.type_declarations.len(), 1);
        assert_eq!(decoded.func_list.len(), 2);
        assert_eq!(decoded.func_bodies.len(), 1);
        assert_eq!(decoded.impl_bodies.len(), 1);
        assert_eq!(decoded.impl_bodies[0].methods.len(), 1);
    }

    #[test]
    fn decode_invalid_magic_fails() {
        let result = decode_bundle(b"XXXX");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid magic"));
    }

    #[test]
    fn decode_truncated_fails() {
        let result = decode_bundle(b"NVIL\x01\x00");
        assert!(result.is_err());
    }
}
