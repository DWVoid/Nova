//! Cross-bundle linker — resolve type and function references across bundles.
//!
//! The linker takes a main [`Bundle`] plus zero or more dependency bundles
//! (loaded from `lib_paths`) and produces a [`LinkedProgram`] with a single
//! merged type table, function table, and constant table.  All indices are
//! remapped so the compiler can emit bytecode with flat, zero-based indices.

use std::collections::HashMap;
use nova_analyze::bundle::{
    Bundle,
    TypeDeclaration, FuncBody, ConstDefinition, ImplBody,
};

/// A fully linked program ready for compilation.
pub struct LinkedProgram {
    /// Display name.
    pub name: String,
    /// Merged type table: FQN → global type index.
    pub types: Vec<String>,
    /// Type declarations, with all indices targeting the merged table.
    pub type_declarations: Vec<TypeDeclaration>,
    /// Merged constant table.
    pub constants: Vec<ConstDefinition>,
    /// Merged function table.
    pub functions: Vec<FuncBody>,
    /// Function display names in the same order as `functions`.
    pub function_names: Vec<String>,
    /// Implementation blocks.
    pub impl_bodies: Vec<ImplBody>,
    /// Map from per-bundle (bundle_idx, local_type_idx) → global type index.
    pub type_map: HashMap<(u32, u32), u32>,
    /// Map from per-bundle (bundle_idx, local_func_idx) → global func index.
    pub func_map: HashMap<(u32, u32), u32>,
}

/// Link a main bundle against its dependencies.
///
/// `main` — the primary bundle (the program entry point).
/// `deps` — dependency bundles, in order (indices 1, 2, 3…).
pub fn link(main: Bundle, deps: Vec<Bundle>) -> LinkedProgram {
    let all_bundles: Vec<&Bundle> = {
        let mut v: Vec<&Bundle> = vec![&main];
        v.extend(deps.iter());
        v
    };

    let mut types: Vec<String> = Vec::new();
    let mut type_map: HashMap<(u32, u32), u32> = HashMap::new();

    // Merge type tables: assign global indices, deduplicate by FQN.
    for (bid, bundle) in all_bundles.iter().enumerate() {
        for (local_idx, entry) in bundle.type_list.iter().enumerate() {
            let key = (bid as u32, local_idx as u32);
            // Check if this FQN is already in the global table.
            if let Some(global_idx) = types.iter().position(|t| t == &entry.name) {
                type_map.insert(key, global_idx as u32);
            } else {
                let global_idx = types.len() as u32;
                types.push(entry.name.clone());
                type_map.insert(key, global_idx);
            }
        }
    }

    // Collect constants (only from self — deps are already compiled).
    let constants: Vec<ConstDefinition> = main.const_definitions.iter()
        .map(|c| {
            let mut c2 = c.clone();
            // Remap type indices in the constant.
            c2.type_index = c.type_index.map(|local_ti| {
                *type_map.get(&(0, local_ti)).unwrap_or(&local_ti)
            });
            c2
        })
        .collect();

    // Collect function names from the main bundle's func_list.
    let function_names: Vec<String> = main.func_list.iter()
        .map(|e| e.name.clone())
        .collect();

    // Collect functions and remap type indices in their bodies.
    let functions: Vec<FuncBody> = main.func_bodies.iter()
        .map(|f| {
            let mut f2 = f.clone();
            f2.semantic = remap_func_body(&f.semantic, &type_map, 0);
            f2
        })
        .collect();

    // Collect impl bodies with remapped indices.
    let impl_bodies: Vec<ImplBody> = main.impl_bodies.iter()
        .map(|ib| {
            let mut ib2 = ib.clone();
            ib2.type_index = *type_map.get(&(0, ib.type_index)).unwrap_or(&ib.type_index);
            ib2.trait_index = ib.trait_index.map(|ti| {
                *type_map.get(&(0, ti)).unwrap_or(&ti)
            });
            ib2.methods = ib2.methods.iter().map(|m| {
                let mut m2 = m.clone();
                m2.semantic = remap_func_body(&m.semantic, &type_map, 0);
                m2
            }).collect();
            ib2
        })
        .collect();

    // Collect type declarations and remap indices.
    let type_declarations: Vec<TypeDeclaration> = main.type_declarations.iter()
        .map(|td| {
            let mut td2 = td.clone();
            td2.type_index = *type_map.get(&(0, td.type_index)).unwrap_or(&td.type_index);
            // Remap field types and variant case types, trait sig return types.
            remap_type_body(&mut td2.body, &type_map, 0);
            td2
        })
        .collect();

    LinkedProgram {
        name: main.metadata.iter()
            .find(|m| m.key == "com.nova.bundle.name")
            .map(|m| String::from_utf8_lossy(&m.value).to_string())
            .unwrap_or_else(|| "unknown".into()),
        types,
        type_declarations,
        constants,
        functions,
        function_names,
        impl_bodies,
        type_map,
        func_map: HashMap::new(),
    }
}

fn remap_func_body(
    body: &nova_analyze::bundle::SemanticFuncBody,
    type_map: &HashMap<(u32, u32), u32>,
    bid: u32,
) -> nova_analyze::bundle::SemanticFuncBody {
    use nova_analyze::bundle::*;
    let params = body.params.iter().map(|p| SemanticParam {
        name: p.name.clone(),
        type_index: p.type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
    }).collect();
    let locals = body.locals.iter().map(|l| SemanticLocal {
        name: l.name.clone(),
        type_index: l.type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
    }).collect();
    let body_block = remap_block(&body.body, type_map, bid);
    SemanticFuncBody { params, return_type: body.return_type, locals, body: body_block }
}

fn remap_block(
    block: &nova_analyze::bundle::SemanticBlock,
    type_map: &HashMap<(u32, u32), u32>,
    bid: u32,
) -> nova_analyze::bundle::SemanticBlock {
    use nova_analyze::bundle::*;
    let locals = block.locals.iter().map(|l| SemanticLocal {
        name: l.name.clone(),
        type_index: l.type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
    }).collect();
    let stmts = block.stmts.iter().map(|s| remap_stmt(s, type_map, bid)).collect();
    SemanticBlock { locals, stmts }
}

fn remap_stmt(
    stmt: &nova_analyze::bundle::SemanticStmt,
    type_map: &HashMap<(u32, u32), u32>,
    bid: u32,
) -> nova_analyze::bundle::SemanticStmt {
    use nova_analyze::bundle::*;
    match stmt {
        SemanticStmt::VarDecl { name, type_index, init, is_var } =>
            SemanticStmt::VarDecl {
                name: name.clone(),
                type_index: type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
                init: init.as_ref().map(|e| Box::new(remap_expr(e, type_map, bid))),
                is_var: *is_var,
            },
        SemanticStmt::Assign { targets, value } => SemanticStmt::Assign {
            targets: targets.iter().map(|e| remap_expr(e, type_map, bid)).collect(),
            value: remap_expr(value, type_map, bid),
        },
        SemanticStmt::Call { expr } => SemanticStmt::Call {
            expr: remap_expr(expr, type_map, bid),
        },
        SemanticStmt::Do(b) => SemanticStmt::Do(remap_block(b, type_map, bid)),
        SemanticStmt::While { cond, body } => SemanticStmt::While {
            cond: remap_expr(cond, type_map, bid),
            body: remap_block(body, type_map, bid),
        },
        SemanticStmt::Repeat { body, until } => SemanticStmt::Repeat {
            body: remap_block(body, type_map, bid),
            until: remap_expr(until, type_map, bid),
        },
        SemanticStmt::If { cond, then, else_ifs, else_block } => SemanticStmt::If {
            cond: remap_expr(cond, type_map, bid),
            then: remap_block(then, type_map, bid),
            else_ifs: else_ifs.iter().map(|(c, b)|
                (remap_expr(c, type_map, bid), remap_block(b, type_map, bid))
            ).collect(),
            else_block: else_block.as_ref().map(|b| remap_block(b, type_map, bid)),
        },
        SemanticStmt::ForNumeric { var, start, end, step, body } => SemanticStmt::ForNumeric {
            var: var.clone(),
            start: remap_expr(start, type_map, bid),
            end: remap_expr(end, type_map, bid),
            step: step.as_ref().map(|e| remap_expr(e, type_map, bid)),
            body: remap_block(body, type_map, bid),
        },
        SemanticStmt::ForGeneric { vars, iter, body } => SemanticStmt::ForGeneric {
            vars: vars.clone(),
            iter: remap_expr(iter, type_map, bid),
            body: remap_block(body, type_map, bid),
        },
        _ => stmt.clone(),
    }
}

fn remap_expr(
    expr: &nova_analyze::bundle::SemanticExpr,
    type_map: &HashMap<(u32, u32), u32>,
    bid: u32,
) -> nova_analyze::bundle::SemanticExpr {
    use nova_analyze::bundle::*;
    match expr {
        SemanticExpr::Lambda { params, return_type, body } => SemanticExpr::Lambda {
            params: params.iter().map(|p| SemanticParam {
                name: p.name.clone(),
                type_index: p.type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
            }).collect(),
            return_type: *return_type,
            body: remap_block(body, type_map, bid),
        },
        SemanticExpr::VarDecl { name, type_index, init, is_var } => SemanticExpr::VarDecl {
            name: name.clone(),
            type_index: type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti)),
            init: init.as_ref().map(|e| Box::new(remap_expr(e, type_map, bid))),
            is_var: *is_var,
        },
        SemanticExpr::Field { object, field } => SemanticExpr::Field {
            object: Box::new(remap_expr(object, type_map, bid)),
            field: field.clone(),
        },
        SemanticExpr::Index { object, index } => SemanticExpr::Index {
            object: Box::new(remap_expr(object, type_map, bid)),
            index: Box::new(remap_expr(index, type_map, bid)),
        },
        SemanticExpr::Call { func, args } => SemanticExpr::Call {
            func: Box::new(remap_expr(func, type_map, bid)),
            args: args.iter().map(|a| remap_expr(a, type_map, bid)).collect(),
        },
        SemanticExpr::Unary { op, expr: e } => SemanticExpr::Unary {
            op: op.clone(),
            expr: Box::new(remap_expr(e, type_map, bid)),
        },
        SemanticExpr::Binary { op, left, right } => SemanticExpr::Binary {
            op: op.clone(),
            left: Box::new(remap_expr(left, type_map, bid)),
            right: Box::new(remap_expr(right, type_map, bid)),
        },
        _ => expr.clone(),
    }
}

fn remap_type_body(
    body: &mut nova_analyze::bundle::TypeBody,
    type_map: &HashMap<(u32, u32), u32>,
    bid: u32,
) {
    use nova_analyze::bundle::*;
    match body {
        TypeBody::Struct { fields } => {
            for f in fields.iter_mut() {
                f.type_index = *type_map.get(&(bid, f.type_index)).unwrap_or(&f.type_index);
            }
        }
        TypeBody::Enum { .. } => {}
        TypeBody::Variant { cases } => {
            for c in cases.iter_mut() {
                c.type_index = *type_map.get(&(bid, c.type_index)).unwrap_or(&c.type_index);
            }
        }
        TypeBody::Trait { signatures } => {
            for s in signatures.iter_mut() {
                for p in s.params.iter_mut() {
                    p.type_index = p.type_index.map(|ti| *type_map.get(&(bid, ti)).unwrap_or(&ti));
                }
                s.return_type = *type_map.get(&(bid, s.return_type)).unwrap_or(&s.return_type);
            }
        }
    }
}
