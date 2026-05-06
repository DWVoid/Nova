//! Compile a linked [`LinkedProgram`] into a [`BytecodeModule`].
//!
//! ## Compilation strategy
//!
//! - Stack-based: every expression leaves a value on the stack.
//! - Statements are compiled for their side effects (no stack contribution).
//! - Variables are local slots (`LOAD` / `STORE`).
//! - Control flow uses `JZ` / `JNZ` / `JMP` with patched jump offsets.
//! - Functions become separate entries in the function table.
//! - The entry point is the first function matching the user-specified name,
//!   falling back to `{bundle_ns}.main`.

use std::collections::HashMap;
use nova_analyze::bundle::{
    SemanticBinaryOp, SemanticExpr, SemanticFuncBody, SemanticStmt, SemanticUnaryOp,
};
use crate::bytecode::{
    BytecodeFunction, BytecodeModule, CodeBuilder, Opcode, Value,
};
use crate::linker::LinkedProgram;
use crate::host::HostFuncDef;

/// Compiled output: bytecode module plus host function descriptors.
pub struct CompiledProgram {
    pub module: BytecodeModule,
    pub host_functions: Vec<HostFuncDef>,
}

/// Compile a linked program into bytecode.
pub fn compile(lp: &LinkedProgram, entry_name: &str, host_funcs: &[HostFuncDef]) -> CompiledProgram {
    let mut constants: Vec<Value> = Vec::new();
    let mut const_map: HashMap<u64, u32> = HashMap::new(); // hash → index

    let _const_index = |v: &Value| -> u32 {
        let h = fxhash(v);
        if let Some(&idx) = const_map.get(&h) { return idx; }
        let idx = constants.len() as u32;
        constants.push(v.clone());
        const_map.insert(h, idx);
        idx
    };

    // Types as strings
    let types: Vec<String> = lp.types.clone();

    // Globals — for now, extract from the bundle metadata or leave empty.
    let globals: Vec<String> = Vec::new();

    // Build function name → index map (for call resolution during compilation).
    // Also insert short names (last segment after the last `.`) so that
    // unqualified calls like `fib(x)` resolve to `Samples.fib`.
    let mut func_name_to_idx: HashMap<String, u32> = HashMap::new();

    for (fi, fb) in lp.functions.iter().enumerate() {
        let fqn = get_func_name(fi, lp);
        func_name_to_idx.insert(fqn.clone(), fi as u32);
        // Short name: everything after the last `.`
        if let Some(dot) = fqn.rfind('.') {
            let short = fqn[dot + 1..].to_string();
            func_name_to_idx.entry(short).or_insert(fi as u32);
        }
    }
    let mut func_offset = lp.functions.len() as u32;
    for ib in &lp.impl_bodies {
        for (mi, _method) in ib.methods.iter().enumerate() {
            let base_type = lp.types.get(ib.type_index as usize)
                .cloned().unwrap_or_else(|| format!("t_{}", ib.type_index));
            let actual_name = format!("{base_type}.method_{mi}");
            func_name_to_idx.insert(actual_name.clone(), func_offset);
            // Short name for method
            if let Some(dot) = actual_name.rfind('.') {
                let short = actual_name[dot + 1..].to_string();
                func_name_to_idx.entry(short).or_insert(func_offset);
            }
            func_offset += 1;
        }
    }

    // Build host function name → index map (with short names).
    let mut host_func_map: HashMap<String, u32> = HashMap::new();
    for (i, f) in host_funcs.iter().enumerate() {
        host_func_map.insert(f.name.clone(), i as u32);
        if let Some(dot) = f.name.rfind('.') {
            let short = f.name[dot + 1..].to_string();
            host_func_map.entry(short).or_insert(i as u32);
        }
    }

    // Compile free functions.
    let mut functions: Vec<BytecodeFunction> = Vec::new();
    for (fi, fb) in lp.functions.iter().enumerate() {
        let fqn = get_func_name(fi, lp);
        let mut compiler = FuncCompiler::new(
            &fqn, &fb.semantic, &lp.type_declarations, &types,
            &mut constants, &mut const_map,
            &func_name_to_idx, &host_func_map,
        );
        let bcf = compiler.compile();
        functions.push(bcf);
    }

    // Compile methods inside impl blocks.
    for ib in &lp.impl_bodies {
        let base_type = lp.types.get(ib.type_index as usize)
            .cloned().unwrap_or_else(|| format!("t_{}", ib.type_index));
        for (mi, method) in ib.methods.iter().enumerate() {
            let actual_name = format!("{base_type}.method_{mi}");
            let mut compiler = FuncCompiler::new(
                &actual_name, &method.semantic, &lp.type_declarations, &types,
                &mut constants, &mut const_map,
                &func_name_to_idx, &host_func_map,
            );
            let bcf = compiler.compile();
            functions.push(bcf);
        }
    }

    // Find entry point.
    let entry_point = func_name_to_idx.get(entry_name)
        // Try `<bundle_name>.main` (from metadata, e.g. "default.main")
        .or_else(|| func_name_to_idx.get(&format!("{}.main", lp.name)))
        // Try bare "main" (short name from any function whose FQN ends in ".main")
        .or_else(|| func_name_to_idx.get("main"))
        // Try the function at index 0
        .or_else(|| func_name_to_idx.values().next())
        .copied()
        .unwrap_or(0);

    CompiledProgram {
        module: BytecodeModule {
            constants,
            globals,
            types,
            functions,
            host_functions: host_funcs.iter().map(|f| crate::bytecode::HostFuncDescriptor {
                name: f.name.clone(),
                arg_count: f.arg_count,
                ret_count: f.ret_count,
            }).collect(),
            entry_point,
        },
        host_functions: host_funcs.to_vec(),
    }
}

fn get_func_name(fi: usize, lp: &LinkedProgram) -> String {
    lp.function_names.get(fi)
        .cloned()
        .unwrap_or_else(|| format!("f_{fi}"))
}

fn fxhash(v: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// ── Per-function compiler ─────────────────────────────────────────────

struct FuncCompiler<'a> {
    name: &'a str,
    body: &'a SemanticFuncBody,
    type_decls: &'a [nova_analyze::bundle::TypeDeclaration],
    types: &'a [String],
    constants: &'a mut Vec<Value>,
    const_map: &'a mut HashMap<u64, u32>,
    cb: CodeBuilder,
    /// For JZ/JNZ/JMP: store (offset_of_offset, target_pc)
    pending_jumps: Vec<(usize, usize)>,
    /// FQN → function index (for bundle function calls).
    func_map: &'a HashMap<String, u32>,
    /// FQN → host function index (for host calls).
    host_map: &'a HashMap<String, u32>,
}

impl<'a> FuncCompiler<'a> {
    fn new(
        name: &'a str,
        body: &'a SemanticFuncBody,
        type_decls: &'a [nova_analyze::bundle::TypeDeclaration],
        types: &'a [String],
        constants: &'a mut Vec<Value>,
        const_map: &'a mut HashMap<u64, u32>,
        func_map: &'a HashMap<String, u32>,
        host_map: &'a HashMap<String, u32>,
    ) -> Self {
        Self {
            name,
            body,
            type_decls,
            types,
            constants,
            const_map,
            cb: CodeBuilder::new(),
            pending_jumps: Vec::new(),
            func_map,
            host_map,
        }
    }

    fn compile(&mut self) -> BytecodeFunction {
        // Compile body statements
        for stmt in &self.body.body.stmts {
            self.compile_stmt(stmt);
        }

        // Ensure we end with a return
        if self.cb.code.last().copied() != Some(Opcode::Return as u8) {
            self.cb.emit(Opcode::Return);
            self.cb.emit_u32(0);
        }

        // Patch pending jumps
        for &(offset_of_offset, target_pc) in &self.pending_jumps {
            let _current_pc = self.cb.offset();
            // The target is relative to after the offset field
            let rel = target_pc as i32 - (offset_of_offset as i32 + 4);
            self.cb.patch_i32(offset_of_offset, rel);
        }

        BytecodeFunction {
            name: self.name.to_string(),
            arg_count: self.body.params.len() as u32,
            ret_count: self.body.return_type.is_some() as u32,
            local_count: self.body.locals.len() as u32 + self.body.params.len() as u32,
            code: self.cb.code.clone(),
        }
    }

    fn compile_stmt(&mut self, stmt: &SemanticStmt) {
        match stmt {
            SemanticStmt::Empty => {}
            SemanticStmt::VarDecl { name, init, .. } => {
                if let Some(init_expr) = init {
                    self.compile_expr(init_expr);
                } else {
                    self.cb.emit(Opcode::Nil);
                }
                let idx = self.resolve_local(name);
                self.cb.emit(Opcode::Store);
                self.cb.emit_u32(idx);
            }
            SemanticStmt::Assign { targets, value } => {
                self.compile_expr(value);
                if let Some(target) = targets.first() {
                    // Simple name assignment → store to local
                    if let SemanticExpr::Name(n) = target {
                        let idx = self.resolve_local(n);
                        self.cb.emit(Opcode::Store);
                        self.cb.emit_u32(idx);
                    } else {
                        // More complex target — compile as expression (consumes value)
                        self.compile_expr(target);
                        self.cb.emit(Opcode::Pop);
                    }
                }
            }
            SemanticStmt::Call { expr } => {
                self.compile_expr(expr);
            }
            SemanticStmt::Do(block) => {
                for s in &block.stmts {
                    self.compile_stmt(s);
                }
            }
            SemanticStmt::While { cond, body } => {
                let loop_start = self.cb.offset();
                self.compile_expr(cond);
                // JZ to after loop
                self.cb.emit(Opcode::Jz);
                let jz_offset = self.cb.offset();
                self.cb.emit_i32(0); // placeholder
                self.pending_jumps.push((jz_offset, 0)); // will be patched

                for s in &body.stmts {
                    self.compile_stmt(s);
                }

                // JMP back to loop start
                self.cb.emit(Opcode::Jmp);
                let jmp_offset = self.cb.offset();
                let back = loop_start as i32 - (jmp_offset as i32 + 4);
                self.cb.emit_i32(back);

                // Patch JZ target to here
                let after_loop = self.cb.offset();
                if let Some(last_pos) = self.pending_jumps.last_mut() {
                    last_pos.1 = after_loop;
                }
            }
            SemanticStmt::Repeat { body, until } => {
                let loop_start = self.cb.offset();
                for s in &body.stmts {
                    self.compile_stmt(s);
                }
                self.compile_expr(until);
                // JZ back to loop start
                self.cb.emit(Opcode::Jz);
                let jz_offset = self.cb.offset();
                let back = loop_start as i32 - (jz_offset as i32 + 4);
                self.cb.emit_i32(back);
            }
            SemanticStmt::If { cond, then, else_ifs, else_block } => {
                let mut patch_offsets: Vec<(usize, usize)> = Vec::new();
                patch_offsets.reserve(1 + else_ifs.len());

                // Compile condition
                self.compile_expr(cond);
                self.cb.emit(Opcode::Jz);
                let jz_off = self.cb.offset(); // where the jump offset is written
                self.cb.emit_i32(0); // placeholder
                patch_offsets.push((jz_off, 0)); // (offset_of_offset, pc_of_target)

                // Compile then branch
                for s in &then.stmts { self.compile_stmt(s); }
                if else_ifs.is_empty() && else_block.is_none() {
                    // Patch JZ to here
                    if let Some(p) = patch_offsets.last_mut() { p.1 = self.cb.offset(); }
                    self.patch_inline(patch_offsets);
                    return;
                }

                // Jump over else blocks
                self.cb.emit(Opcode::Jmp);
                let jmp_off = self.cb.offset();
                self.cb.emit_i32(0); // placeholder
                patch_offsets.push((jmp_off, 0));

                // Patch JZ to here (start of else/else-if)
                if let Some(p) = patch_offsets.first_mut() { p.1 = self.cb.offset(); }

                // Compile else-if chains
                for (econd, eblock) in else_ifs {
                    self.compile_expr(econd);
                    self.cb.emit(Opcode::Jz);
                    let ejz_off = self.cb.offset();
                    self.cb.emit_i32(0);
                    let mut extra = vec![(ejz_off, 0)];
                    for s in &eblock.stmts { self.compile_stmt(s); }
                    // Jump over remaining else blocks
                    self.cb.emit(Opcode::Jmp);
                    let ejmp_off = self.cb.offset();
                    self.cb.emit_i32(0);
                    extra.push((ejmp_off, 0));
                    if let Some(p) = extra.first_mut() { p.1 = self.cb.offset(); }
                    patch_offsets.extend(extra);
                }

                // Compile else block
                if let Some(eb) = else_block {
                    for s in &eb.stmts { self.compile_stmt(s); }
                }

                let after_if = self.cb.offset();
                for p in patch_offsets.iter_mut() {
                    if p.1 == 0 { p.1 = after_if; }
                    let rel = p.1 as i32 - (p.0 as i32 + 4);
                    self.cb.patch_i32(p.0, rel);
                }
            }
            SemanticStmt::ForNumeric { var, start, end, step, body } => {
                // var = start
                self.compile_expr(start);
                let var_idx = self.resolve_local(var);
                self.cb.emit(Opcode::Store);
                self.cb.emit_u32(var_idx);

                let loop_start = self.cb.offset();
                // Check condition: var <= end
                self.cb.emit(Opcode::Load);
                self.cb.emit_u32(var_idx);
                self.compile_expr(end);
                self.cb.emit(Opcode::Gt);
                // If var > end, exit
                self.cb.emit(Opcode::Jnz);
                let exit_off = self.cb.offset();
                self.cb.emit_i32(0);
                self.pending_jumps.push((exit_off, 0));

                for s in &body.stmts { self.compile_stmt(s); }

                // var += step (or 1)
                self.cb.emit(Opcode::Load);
                self.cb.emit_u32(var_idx);
                if let Some(sv) = step {
                    self.compile_expr(sv);
                } else {
                    let ci = self.add_const(Value::Number(1.0));
                    self.cb.emit(Opcode::Const);
                    self.cb.emit_u32(ci);
                }
                self.cb.emit(Opcode::Add);
                self.cb.emit(Opcode::Store);
                self.cb.emit_u32(var_idx);

                // Loop back
                self.cb.emit(Opcode::Jmp);
                let back_off = self.cb.offset();
                let back = loop_start as i32 - (back_off as i32 + 4);
                self.cb.emit_i32(back);

                // Patch exit
                if let Some(sp) = self.pending_jumps.last_mut() {
                    sp.1 = self.cb.offset();
                }
            }
            SemanticStmt::ForGeneric { vars: _, iter, body } => {
                // For generic iteration: for now, evaluate iter and ignore vars
                self.compile_expr(iter);
                self.cb.emit(Opcode::Pop);
                for s in &body.stmts { self.compile_stmt(s); }
            }
            SemanticStmt::Break | SemanticStmt::Continue => {
                // Stub — no loop context tracking yet
            }
            SemanticStmt::Return { values } => {
                for v in values { self.compile_expr(v); }
                self.cb.emit(Opcode::Return);
                self.cb.emit_u32(values.len() as u32);
            }
            SemanticStmt::Label(_) => {}
            SemanticStmt::Goto(_) => {}
        }
    }

    fn compile_expr(&mut self, expr: &SemanticExpr) {
        match expr {
            SemanticExpr::Nil => self.cb.emit(Opcode::Nil),
            SemanticExpr::Bool(b) => {
                if *b { self.cb.emit(Opcode::True); }
                else { self.cb.emit(Opcode::False); }
            }
            SemanticExpr::Number(n) => {
                let idx = self.add_const(Value::Number(n.parse().unwrap_or(0.0)));
                self.cb.emit(Opcode::Const);
                self.cb.emit_u32(idx);
            }
            SemanticExpr::String(s) => {
                let idx = self.add_const(Value::String(s.clone()));
                self.cb.emit(Opcode::Const);
                self.cb.emit_u32(idx);
            }
            SemanticExpr::Name(n) => {
                let idx = self.resolve_local(n);
                self.cb.emit(Opcode::Load);
                self.cb.emit_u32(idx);
            }
            SemanticExpr::Field { object, field } => {
                self.compile_expr(object);
                // Field access — for now just pop the object and push nil
                self.cb.emit(Opcode::Pop);
                self.cb.emit(Opcode::Nil);
            }
            SemanticExpr::Index { object, index } => {
                self.compile_expr(object);
                self.compile_expr(index);
                // Stub — push nil
                self.cb.emit(Opcode::Pop);
                self.cb.emit(Opcode::Nil);
            }
            SemanticExpr::Call { func, args } => {
                // Resolve the call target name to a FQN.
                let target_fqn = match func.as_ref() {
                    SemanticExpr::Name(n) => Some(n.clone()),
                    SemanticExpr::Field { object, field } => {
                        if let SemanticExpr::Name(ns) = object.as_ref() {
                            Some(format!("{ns}.{field}"))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                if let Some(fqn) = target_fqn {
                    // Check host functions first.
                    if let Some(hi) = self.host_map.get(&fqn) {
                        for a in args { self.compile_expr(a); }
                        self.cb.emit(Opcode::CallHost);
                        self.cb.emit_u32(*hi);
                        self.cb.emit_u32(args.len() as u32);
                        return;
                    }
                    // Check bundle functions.
                    if let Some(fi) = self.resolve_func(&fqn) {
                        for a in args { self.compile_expr(a); }
                        self.cb.emit(Opcode::Call);
                        self.cb.emit_u32(fi);
                        self.cb.emit_u32(args.len() as u32);
                        return;
                    }
                }
                // Fallback.
                self.compile_expr(func);
                for a in args { self.compile_expr(a); }
                self.cb.emit(Opcode::Pop);
                self.cb.emit(Opcode::Nil);
            }
            SemanticExpr::Unary { op, expr: e } => {
                self.compile_expr(e);
                let bc_op = match op {
                    SemanticUnaryOp::Neg => Opcode::Neg,
                    SemanticUnaryOp::Not => Opcode::Not,
                    SemanticUnaryOp::Len => Opcode::Nop,  // stub
                    SemanticUnaryOp::BitNot => Opcode::BNot,
                };
                self.cb.emit(bc_op);
            }
            SemanticExpr::Binary { op, left, right } => {
                self.compile_expr(left);
                self.compile_expr(right);
                let bc_op = match op {
                    SemanticBinaryOp::Add => Opcode::Add,
                    SemanticBinaryOp::Sub => Opcode::Sub,
                    SemanticBinaryOp::Mul => Opcode::Mul,
                    SemanticBinaryOp::Div => Opcode::Div,
                    SemanticBinaryOp::FloorDiv => Opcode::Div,
                    SemanticBinaryOp::Mod => Opcode::Mod,
                    SemanticBinaryOp::Pow => Opcode::Pow,
                    SemanticBinaryOp::Eq => Opcode::Eq,
                    SemanticBinaryOp::NotEq => Opcode::Neq,
                    SemanticBinaryOp::Less => Opcode::Lt,
                    SemanticBinaryOp::LessEq => Opcode::Le,
                    SemanticBinaryOp::Greater => Opcode::Gt,
                    SemanticBinaryOp::GreaterEq => Opcode::Ge,
                    SemanticBinaryOp::And => Opcode::And,
                    SemanticBinaryOp::Or => Opcode::Or,
                    SemanticBinaryOp::Concat => Opcode::Concat,
                    SemanticBinaryOp::BitAnd => Opcode::BAnd,
                    SemanticBinaryOp::BitOr => Opcode::BOr,
                    SemanticBinaryOp::BitXor => Opcode::BXor,
                    SemanticBinaryOp::Shl => Opcode::Shl,
                    SemanticBinaryOp::Shr => Opcode::Shr,
                };
                self.cb.emit(bc_op);
            }
            SemanticExpr::Lambda { params, return_type, body } => {
                // Push a function reference — for now a placeholder
                self.cb.emit(Opcode::Nil);
            }
            SemanticExpr::VarDecl { name, init, .. } => {
                if let Some(e) = init {
                    self.compile_expr(e);
                } else {
                    self.cb.emit(Opcode::Nil);
                }
                // Store and re-load (so expression leaves value on stack)
                let idx = self.resolve_local(name);
                self.cb.emit(Opcode::Dup);
                self.cb.emit(Opcode::Store);
                self.cb.emit_u32(idx);
            }
        }
    }

    fn add_const(&mut self, v: Value) -> u32 {
        let h = fxhash(&v);
        if let Some(&idx) = self.const_map.get(&h) { return idx; }
        let idx = self.constants.len() as u32;
        self.constants.push(v);
        self.const_map.insert(h, idx);
        idx
    }

    fn resolve_local(&self, name: &str) -> u32 {
        // Params come first, then locals
        for (i, p) in self.body.params.iter().enumerate() {
            if p.name == name { return i as u32; }
        }
        for (i, l) in self.body.locals.iter().enumerate() {
            if l.name == name { return (self.body.params.len() + i) as u32; }
        }
        // Not found — assume it's a local at the end
        (self.body.params.len() + self.body.locals.len()) as u32
    }

    fn resolve_func(&self, name: &str) -> Option<u32> {
        self.func_map.get(name).copied()
    }

    fn patch_inline(&mut self, patches: Vec<(usize, usize)>) {
        let after = self.cb.offset();
        for (offset_of_offset, target_pc) in patches {
            let target = if target_pc == 0 { after } else { target_pc };
            let rel = target as i32 - (offset_of_offset as i32 + 4);
            self.cb.patch_i32(offset_of_offset, rel);
        }
    }
}
