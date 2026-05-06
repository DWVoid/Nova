//! Stack-based bytecode VM for Nova.
//!
//! ## Execution model
//!
//! - **Single flat loop** — all instructions are dispatched from one loop.
//!   Call and Return manipulate the frame stack directly; the VM never
//!   recurses or uses host-stack frames.  This design enables future
//!   preemptive scheduling (pause/resume at any instruction boundary).
//!
//! - **Value stack** — shared across all frames, holds intermediates and
//!   return values.  Each frame's `base` pointer splits the value stack
//!   into the frame's local region and the free area above it.
//!
//! - **Frame stack** — metadata-only: function index, program counter,
//!   value-stack base pointer.  Calls push a new frame; Returns pop it.
//!
//! - **Host functions** — called through the same instruction dispatch loop.
//!   `CALL_HOST` evaluates arguments on the value stack, invokes the
//!   registered native callback, and pushes results back.  No host-stack
//!   recursion.
//!
//! - **Globals** — a `HashMap<String, Value>` shared across all frames.

use std::collections::HashMap;
use crate::bytecode::{BytecodeModule, Opcode, Value, disassemble};
use crate::host::HostFuncDef;

/// The Nova VM — executes a [`BytecodeModule`] with a fully emulated
/// call stack (no host-stack recursion).
pub struct Vm {
    module: BytecodeModule,
    /// Global variable store.
    globals: HashMap<String, Value>,
    /// Registered host function implementations.
    host_funcs: Vec<HostFuncDef>,
    /// Shared value stack (all frames).
    stack: Vec<Value>,
    /// Emulated frame metadata stack.
    frames: Vec<Frame>,

    /// Debug: print a trace line per instruction.
    pub trace: bool,
}

/// Per-frame metadata.  All mutable state lives here or on the value stack.
struct Frame {
    /// Index into `module.functions[]`.
    func_idx: u32,
    /// Program counter into the function's `code` array.
    pc: usize,
    /// Index into `self.stack` where this frame's locals begin.
    base: usize,
}

impl Vm {
    /// Create a new VM.
    pub fn new(module: BytecodeModule, host_funcs: Vec<HostFuncDef>) -> Self {
        Self {
            module,
            globals: HashMap::new(),
            host_funcs,
            stack: Vec::new(),
            frames: Vec::new(),
            trace: false,
        }
    }

    pub fn set_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    /// Return the display name of the entry-point function.
    pub fn entry_name(&self, entry: u32) -> &str {
        self.module.functions.get(entry as usize)
            .map(|f| f.name.as_str())
            .unwrap_or("<unknown>")
    }

    /// Run from `entry` function.  Single flat dispatch loop.
    ///
    /// Returns the values left on the stack after the entry function returns.
    pub fn run(&mut self, entry: u32) -> Result<Vec<Value>, VmError> {
        self.trace = false;
        if (entry as usize) >= self.module.functions.len() {
            return Err(VmError::new(format!("entry function {entry} not found")));
        }

        // Seed the initial frame with locals pre-allocated.
        let entry_func = &self.module.functions[entry as usize];
        let initial_base = self.stack.len();
        for _ in 0..entry_func.local_count as usize {
            self.stack.push(Value::Nil);
        }
        self.frames.push(Frame { func_idx: entry, pc: 0, base: initial_base });

        while let Some(fi) = self.frames.last().map(|f| f.func_idx) {
            let func = &self.module.functions[fi as usize];
            let code = &func.code;
            let pc = self.frames.last().unwrap().pc;

            if pc >= code.len() {
                return Err(VmError::new(format!(
                    "pc {pc} out of bounds (code len {})", code.len()
                )));
            }

            if self.trace {
                let (mnem, ops, _) = disassemble(code, pc)
                    .unwrap_or_else(|_| ("??".into(), "".into(), 1));
                print!("  {:04x}  {mnem} {ops}", pc);
                println!("  [stack: {} frame: {}]", self.stack.len(), fi);
            }

            let op_byte = code[pc];
            let op = Opcode::try_from(op_byte)
                .map_err(|_| VmError::new(format!("bad opcode 0x{op_byte:02X} at {pc}")))?;

            match op {
                // ── Stack literals ──────────────────────────────────────
                Opcode::Nil => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Nil);
                }
                Opcode::True => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Bool(true));
                }
                Opcode::False => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Bool(false));
                }
                Opcode::I8 => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = code[self.frames.last().unwrap().pc] as i8;
                    self.frames.last_mut().unwrap().pc += 1;
                    self.stack.push(Value::Number(val as f64));
                }
                Opcode::I32 => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = read_i32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Number(val as f64));
                }
                Opcode::F64 => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = read_f64(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Number(val));
                }
                Opcode::String => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let s = read_str(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::String(s));
                }
                Opcode::Dup => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let v = self.stack.last().ok_or(VmError::stack_underflow())?.clone();
                    self.stack.push(v);
                }
                Opcode::Pop => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.pop().ok_or(VmError::stack_underflow())?;
                }
                Opcode::Const => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let idx = read_u32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = self.module.constants.get(idx as usize)
                        .ok_or_else(|| VmError::new(format!("const {idx} out of bounds")))?
                        .clone();
                    let val = self.module.constants.get(idx as usize)
                        .ok_or_else(|| VmError::new(format!("const {idx} out of bounds")))?
                        .clone();
                    self.stack.push(val);
                }
                Opcode::Load => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let idx = read_u32(code, &mut self.frames.last_mut().unwrap().pc)? as usize;
                    let target = self.frames.last().unwrap().base + idx;
                    let val = self.stack.get(target).ok_or_else(|| VmError::new("local not found"))?.clone();
                    self.stack.push(val);
                }
                Opcode::Store => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let idx = read_u32(code, &mut self.frames.last_mut().unwrap().pc)? as usize;
                    let target = self.frames.last().unwrap().base + idx;
                    let val = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    if target >= self.stack.len() {
                        self.stack.resize(target + 1, Value::Nil);
                    }
                    self.stack[target] = val;
                }
                Opcode::GLoad => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let name = read_str(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = self.globals.get(&name).cloned().unwrap_or(Value::Nil);
                    self.stack.push(val);
                }
                Opcode::GStore => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let name = read_str(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let val = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    self.globals.insert(name, val);
                }

                // ── Calls ──────────────────────────────────────────────
                Opcode::Call => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let fi = read_u32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let arg_count = read_u32(code, &mut self.frames.last_mut().unwrap().pc)? as usize;

                    if (fi as usize) >= self.module.functions.len() {
                        return Err(VmError::new(format!("function {fi} not found")));
                    }

                    let func = &self.module.functions[fi as usize];
                    let new_base = self.stack.len() - arg_count;

                    // Allocate nil slots for local variables beyond args.
                    let extra_locals = (func.local_count as usize).saturating_sub(arg_count);
                    for _ in 0..extra_locals {
                        self.stack.push(Value::Nil);
                    }

                    self.frames.push(Frame { func_idx: fi, pc: 0, base: new_base });
                }
                Opcode::CallHost => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let hi = read_u32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let arg_count = read_u32(code, &mut self.frames.last_mut().unwrap().pc)? as usize;

                    let hf = self.host_funcs.get(hi as usize)
                        .ok_or_else(|| VmError::new(format!("host function {hi} not found")))?;

                    let args_start = self.stack.len() - arg_count;
                    let args: Vec<Value> = self.stack.drain(args_start..).collect();
                    let results = (hf.native)(&args)
                        .map_err(|e| VmError::new(format!("host error: {e}")))?;
                    self.stack.extend(results);
                }
                Opcode::Return => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let ret_count = read_u32(code, &mut self.frames.last_mut().unwrap().pc)? as usize;

                    // Drain return values from the top of the stack.
                    let returns: Vec<Value> = if ret_count > 0 {
                        let start = self.stack.len().saturating_sub(ret_count);
                        self.stack.drain(start..).collect()
                    } else {
                        vec![]
                    };

                    // Save the returning frame's base BEFORE popping — we need
                    // it to truncate the stack back to just the caller's area.
                    let child_base = self.frames.last().unwrap().base;
                    self.frames.pop();

                    if self.frames.is_empty() {
                        self.stack.clear();
                        self.stack.extend(returns);
                        break;
                    }

                    // Truncate to the CHILD's base (not the caller's).  This
                    // removes the child's args, locals, and intermediates but
                    // preserves the caller's local variables.
                    self.stack.truncate(child_base);
                    self.stack.extend(returns);
                }
                Opcode::Closure => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let fi = read_u32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Function(fi));
                }

                // ── Arithmetic ──────────────────────────────────────────
                Opcode::Add => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a + b)?; }
                Opcode::Sub => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a - b)?; }
                Opcode::Mul => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a * b)?; }
                Opcode::Div => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a / b)?; }
                Opcode::Mod => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a % b)?; }
                Opcode::Pow => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; bin_op_num(&mut self.stack, |a, b| a.powf(b))?; }
                Opcode::Neg => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let a = pop_number(&mut self.stack)?;
                    self.stack.push(Value::Number(-a));
                }

                // ── Comparison ─────────────────────────────────────────
                Opcode::Eq  => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; cmp_op_num(&mut self.stack, |a, b| (a - b).abs() < f64::EPSILON)?; }
                Opcode::Neq => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; cmp_op_num(&mut self.stack, |a, b| (a - b).abs() >= f64::EPSILON)?; }
                Opcode::Lt  => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;

                    cmp_op_num(&mut self.stack, |a, b| a < b)?;
                }
                Opcode::Le  => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; cmp_op_num(&mut self.stack, |a, b| a <= b)?; }
                Opcode::Gt  => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; cmp_op_num(&mut self.stack, |a, b| a > b)?; }
                Opcode::Ge  => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; cmp_op_num(&mut self.stack, |a, b| a >= b)?; }

                // ── Logical ────────────────────────────────────────────
                Opcode::Not => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let v = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    self.stack.push(Value::Bool(!v.is_truthy()));
                }
                Opcode::And => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let b = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    let a = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    self.stack.push(Value::Bool(a.is_truthy() && b.is_truthy()));
                }
                Opcode::Or => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let b = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    let a = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    self.stack.push(Value::Bool(a.is_truthy() || b.is_truthy()));
                }
                Opcode::Concat => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let b = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    let a = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    let a_str = format_value(&a);
                    let b_str = format_value(&b);
                    self.stack.push(Value::String(a_str + &b_str));
                }

                // ── Bitwise ────────────────────────────────────────────
                Opcode::BAnd => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; int_bin_op(&mut self.stack, |a, b| a & b)?; }
                Opcode::BOr  => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; int_bin_op(&mut self.stack, |a, b| a | b)?; }
                Opcode::BXor => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; int_bin_op(&mut self.stack, |a, b| a ^ b)?; }
                Opcode::BNot => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let a = pop_int(&mut self.stack)?;
                    self.stack.push(Value::Number((!a) as f64));
                }
                Opcode::Shl => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; int_bin_op(&mut self.stack, |a, b| a << b)?; }
                Opcode::Shr => { advance(code, &mut self.frames.last_mut().unwrap().pc)?; int_bin_op(&mut self.stack, |a, b| a >> b)?; }

                // ── Control flow ───────────────────────────────────────
                // Offsets are computed by the compiler relative to AFTER the
                // jump instruction (i.e. `target - (jz_off + 4)` where `jz_off`
                // is the position of the i32 operand, so the offset measures
                // distance from the byte after the whole JMP/JZ/JNZ).
                // We apply the offset from the pc AFTER advancing past it.
                Opcode::Jmp => {
                    let pc_after = self.frames.last().unwrap().pc;
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let offset = read_i32(code, &mut self.frames.last_mut().unwrap().pc)?; // now pc = pc_after + 5
                    // pc_after already points to the JMP opcode. After advance+read_i32,
                    // pc points past the entire instruction. Apply offset from here.
                    self.frames.last_mut().unwrap().pc = (self.frames.last().unwrap().pc as i32 + offset) as usize;
                }
                Opcode::Jz => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let offset = read_i32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let cond = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    if !cond.is_truthy() {
                        self.frames.last_mut().unwrap().pc = (self.frames.last().unwrap().pc as i32 + offset) as usize;
                    }
                }
                Opcode::Jnz => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let offset = read_i32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let cond = self.stack.pop().ok_or(VmError::stack_underflow())?;
                    if cond.is_truthy() {
                        self.frames.last_mut().unwrap().pc = (self.frames.last().unwrap().pc as i32 + offset) as usize;
                    }
                }
                Opcode::Loop => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let offset = read_i32(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let base = self.frames.last().unwrap().base;
                    if let Some(Value::Number(n)) = self.stack.get_mut(base) {
                        if *n > 0.0 {
                            *n -= 1.0;
                            self.frames.last_mut().unwrap().pc = (self.frames.last().unwrap().pc as i32 + offset) as usize;
                        }
                    }
                }

                // ── Misc ────────────────────────────────────────────────
                Opcode::Nop => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                }
                Opcode::Breakpoint => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    let bp_pc = self.frames.last().unwrap().pc - 1;
                    println!("[debug] breakpoint at pc {bp_pc}");
                }
                Opcode::Halt => break,

                Opcode::NewTable | Opcode::TGet | Opcode::TSet => {
                    advance(code, &mut self.frames.last_mut().unwrap().pc)?;
                    self.stack.push(Value::Nil);
                }
            }
        }

        // Collect whatever is left on the stack (return values).
        Ok(self.stack.drain(..).collect())
    }
}

// ── Operand read helpers ──────────────────────────────────────────────
//
// Each match arm calls `advance()` to skip the opcode byte first, then
// calls the read functions for operands.  The read functions never skip
// an opcode byte — only numeric payload bytes.

/// Advance `*pc` past the opcode byte.
fn advance(code: &[u8], pc: &mut usize) -> Result<(), VmError> {
    if *pc >= code.len() {
        return Err(VmError::new("pc past end of code"));
    }
    *pc += 1;
    Ok(())
}

/// Read a `u32` operand at `*pc`, advance `*pc` by 4.
fn read_u32(code: &[u8], pc: &mut usize) -> Result<u32, VmError> {
    if *pc + 4 > code.len() {
        return Err(VmError::new("truncated u32 operand"));
    }
    let b = &code[*pc..*pc + 4];
    *pc += 4;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read an `i32` operand at `*pc`, advance `*pc` by 4.
fn read_i32(code: &[u8], pc: &mut usize) -> Result<i32, VmError> {
    if *pc + 4 > code.len() {
        return Err(VmError::new("truncated i32 operand"));
    }
    let b = &code[*pc..*pc + 4];
    *pc += 4;
    Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read an `f64` operand at `*pc`, advance `*pc` by 8.
fn read_f64(code: &[u8], pc: &mut usize) -> Result<f64, VmError> {
    if *pc + 8 > code.len() {
        return Err(VmError::new("truncated f64 operand"));
    }
    let b = &code[*pc..*pc + 8];
    *pc += 8;
    Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// Read a length-prefixed string at `*pc`, advance `*pc`.
fn read_str(code: &[u8], pc: &mut usize) -> Result<String, VmError> {
    if *pc + 4 > code.len() {
        return Err(VmError::new("truncated string length"));
    }
    let len = u32::from_le_bytes([code[*pc], code[*pc + 1], code[*pc + 2], code[*pc + 3]]) as usize;
    *pc += 4;
    if *pc + len > code.len() {
        return Err(VmError::new("truncated string data"));
    }
    let s = std::str::from_utf8(&code[*pc..*pc + len])
        .map_err(|e| VmError::new(format!("invalid utf8: {e}")))?
        .to_string();
    *pc += len;
    Ok(s)
}

// ── Stack helpers ─────────────────────────────────────────────────────

fn pop_number(stack: &mut Vec<Value>) -> Result<f64, VmError> {
    match stack.pop().ok_or(VmError::stack_underflow())? {
        Value::Number(n) => Ok(n),
        v => Err(VmError::new(format!("expected number, got {v:?}"))),
    }
}

fn pop_int(stack: &mut Vec<Value>) -> Result<i64, VmError> {
    match stack.pop().ok_or(VmError::stack_underflow())? {
        Value::Number(n) => Ok(n as i64),
        v => Err(VmError::new(format!("expected number, got {v:?}"))),
    }
}

fn bin_op_num(stack: &mut Vec<Value>, op: fn(f64, f64) -> f64) -> Result<(), VmError> {
    let b = pop_number(stack)?;
    let a = pop_number(stack)?;
    stack.push(Value::Number(op(a, b)));
    Ok(())
}

fn cmp_op_num(stack: &mut Vec<Value>, op: fn(f64, f64) -> bool) -> Result<(), VmError> {
    let b = pop_number(stack)?;
    let a = pop_number(stack)?;
    stack.push(Value::Bool(op(a, b)));
    Ok(())
}

fn int_bin_op(stack: &mut Vec<Value>, op: fn(i64, i64) -> i64) -> Result<(), VmError> {
    let b = pop_int(stack)?;
    let a = pop_int(stack)?;
    stack.push(Value::Number(op(a, b) as f64));
    Ok(())
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() { format!("{}", *n as i64) }
            else { format!("{n}") }
        }
        Value::String(s) => s.clone(),
        Value::Function(_) => "<func>".into(),
    }
}

// ── Error ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmError {
    pub message: String,
}

impl VmError {
    pub fn new(msg: impl Into<String>) -> Self { Self { message: msg.into() } }
    fn stack_underflow() -> Self { Self { message: "stack underflow".into() } }
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VM: {}", self.message)
    }
}
impl std::error::Error for VmError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{BytecodeFunction, BytecodeModule, CodeBuilder};

    fn make_module(code: Vec<u8>, constants: Vec<Value>) -> BytecodeModule {
        BytecodeModule {
            constants,
            globals: vec![],
            types: vec![],
            functions: vec![BytecodeFunction {
                name: "test".into(), arg_count: 0, ret_count: 1, local_count: 0, code,
            }],
            host_functions: vec![],
            entry_point: 0,
        }
    }

    #[test]
    fn push_const_return() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);
        let m = make_module(cb.code, vec![Value::Number(42.0)]);
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        assert_eq!(results, vec![Value::Number(42.0)]);
    }

    #[test]
    fn add_two_numbers() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Const);
        cb.emit_u32(1);
        cb.emit(Opcode::Add);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);
        let m = make_module(cb.code, vec![Value::Number(10.0), Value::Number(32.0)]);
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        assert_eq!(results, vec![Value::Number(42.0)]);
    }

    #[test]
    fn locals_store_and_load() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Store);
        cb.emit_u32(0);
        cb.emit(Opcode::Load);
        cb.emit_u32(0);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);
        let m = BytecodeModule {
            constants: vec![Value::Number(42.0)],
            globals: vec![],
            types: vec![],
            functions: vec![BytecodeFunction {
                name: "test".into(), arg_count: 0, ret_count: 1, local_count: 1, code: cb.code,
            }],
            host_functions: vec![],
            entry_point: 0,
        };
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        assert_eq!(results, vec![Value::Number(42.0)]);
    }

    #[test]
    fn call_and_return() {
        // Function 0 (main): calls function 1 with no args, returns the result.
        let mut main = CodeBuilder::new();
        main.emit(Opcode::Call);
        main.emit_u32(1);    // func_index = 1
        main.emit_u32(0);    // 0 arguments
        main.emit(Opcode::Return);
        main.emit_u32(1);    // 1 return value

        // Function 1 (helper): pushes constant 99, returns.
        let mut helper = CodeBuilder::new();
        helper.emit(Opcode::Const);
        helper.emit_u32(0);
        helper.emit(Opcode::Return);
        helper.emit_u32(1);

        let m = BytecodeModule {
            constants: vec![Value::Number(99.0)],
            globals: vec![],
            types: vec![],
            functions: vec![
                BytecodeFunction {
                    name: "main".into(), arg_count: 0, ret_count: 1, local_count: 0,
                    code: main.code,
                },
                BytecodeFunction {
                    name: "helper".into(), arg_count: 0, ret_count: 1, local_count: 0,
                    code: helper.code,
                },
            ],
            host_functions: vec![],
            entry_point: 0,
        };
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        assert_eq!(results, vec![Value::Number(99.0)]);
    }

    #[test]
    fn halt_stops_execution() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Halt);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);
        let m = make_module(cb.code, vec![Value::Number(99.0)]);
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        // HALT stops before RETURN; one value should be on stack.
        assert!(results.len() >= 1);
    }

    #[test]
    fn jmp_skip_middle() {
        // Push 1, JMP over push 2, push 3, return. Result should be 1 then 3.
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);           // push 1
        cb.emit(Opcode::Jmp);
        cb.emit_i32(2 * 5);       // skip push 2 (CONST + u32 + JMP + i32 = 10 bytes)
        cb.emit(Opcode::Const);
        cb.emit_u32(1);           // push 2 (skipped)
        cb.emit(Opcode::Const);
        cb.emit_u32(2);           // push 3
        cb.emit(Opcode::Return);
        cb.emit_u32(2);           // return both values
        let m = make_module(cb.code, vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
        let mut vm = Vm::new(m, vec![]);
        let results = vm.run(0).unwrap();
        assert_eq!(results, vec![Value::Number(1.0), Value::Number(3.0)]);
    }
}
