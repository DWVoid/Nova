//! # Nova Bytecode (NVBC) — types, encoding, decoding
//!
//! ## Overview
//!
//! Stack-based bytecode with fixed-size opcodes.  Each function has a
//! flat code array, a constant pool, a local count, and argument/return
//! counts.  The module packages all functions together with the type
//! and global tables.

use std::io::{Cursor, Read};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes at the start of every serialised NVBC module.
pub const NVBC_MAGIC: &[u8; 4] = b"NVBC";

/// Current bytecode format version.
pub const NVBC_VERSION_MAJOR: u16 = 0;
pub const NVBC_VERSION_MINOR: u16 = 1;

// ---------------------------------------------------------------------------
// Value type
// ---------------------------------------------------------------------------

/// A runtime value in the Nova VM.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    /// Index into the module's function table (for closures / function refs).
    Function(u32),
}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Value::Nil => 0u8.hash(state),
            Value::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Value::Number(n) => {
                2u8.hash(state);
                n.to_bits().hash(state);
            }
            Value::String(s) => {
                3u8.hash(state);
                s.hash(state);
            }
            Value::Function(fi) => {
                4u8.hash(state);
                fi.hash(state);
            }
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Function(_) => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Instructions
// ---------------------------------------------------------------------------

/// Opcodes for the NVBC instruction set.
///
/// Each opcode is a single byte. Operands follow immediately and have
/// sizes determined by the opcode.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Opcode {
    // Stack literals
    Nil = 0x00,
    True = 0x01,
    False = 0x02,
    Const = 0x03,       // u32 idx → push constant
    I8 = 0x04,          // i8 → push
    I32 = 0x05,         // i32 → push
    F64 = 0x06,         // f64 LE → push
    String = 0x07,      // u32 len + bytes → push
    Dup = 0x08,
    Pop = 0x09,

    // Variables
    Load = 0x10,        // u32 local_idx
    Store = 0x11,       // u32 local_idx
    GLoad = 0x12,       // str global_name
    GStore = 0x13,      // str global_name

    // Calls
    Call = 0x20,        // u32 func_idx, u32 arg_count
    CallHost = 0x21,    // u32 host_func_idx, u32 arg_count
    Return = 0x22,      // u32 ret_count
    Closure = 0x23,     // u32 func_idx

    // Tables
    NewTable = 0x30,
    TGet = 0x31,
    TSet = 0x32,

    // Arithmetic
    Add = 0x40,
    Sub = 0x41,
    Mul = 0x42,
    Div = 0x43,
    Mod = 0x44,
    Pow = 0x45,
    Neg = 0x46,

    // Comparison
    Eq = 0x48,
    Neq = 0x49,
    Lt = 0x4A,
    Le = 0x4B,
    Gt = 0x4C,
    Ge = 0x4D,

    // Logical
    Not = 0x50,
    And = 0x51,
    Or = 0x52,
    Concat = 0x53,

    // Bitwise
    BAnd = 0x58,
    BOr = 0x59,
    BXor = 0x5A,
    BNot = 0x5B,
    Shl = 0x5C,
    Shr = 0x5D,

    // Control flow
    Jmp = 0x60,         // i32 offset
    Jz = 0x61,          // i32 offset (jump if false)
    Jnz = 0x62,         // i32 offset (jump if true)
    Loop = 0x63,        // i32 offset (decrement counter, jump)

    // Misc
    Nop = 0xF0,
    Breakpoint = 0xF1,
    Halt = 0xFF,
}

impl TryFrom<u8> for Opcode {
    type Error = String;
    fn try_from(byte: u8) -> Result<Self, String> {
        match byte {
            0x00 => Ok(Opcode::Nil),
            0x01 => Ok(Opcode::True),
            0x02 => Ok(Opcode::False),
            0x03 => Ok(Opcode::Const),
            0x04 => Ok(Opcode::I8),
            0x05 => Ok(Opcode::I32),
            0x06 => Ok(Opcode::F64),
            0x07 => Ok(Opcode::String),
            0x08 => Ok(Opcode::Dup),
            0x09 => Ok(Opcode::Pop),
            0x10 => Ok(Opcode::Load),
            0x11 => Ok(Opcode::Store),
            0x12 => Ok(Opcode::GLoad),
            0x13 => Ok(Opcode::GStore),
            0x20 => Ok(Opcode::Call),
            0x21 => Ok(Opcode::CallHost),
            0x22 => Ok(Opcode::Return),
            0x23 => Ok(Opcode::Closure),
            0x30 => Ok(Opcode::NewTable),
            0x31 => Ok(Opcode::TGet),
            0x32 => Ok(Opcode::TSet),
            0x40 => Ok(Opcode::Add),
            0x41 => Ok(Opcode::Sub),
            0x42 => Ok(Opcode::Mul),
            0x43 => Ok(Opcode::Div),
            0x44 => Ok(Opcode::Mod),
            0x45 => Ok(Opcode::Pow),
            0x46 => Ok(Opcode::Neg),
            0x48 => Ok(Opcode::Eq),
            0x49 => Ok(Opcode::Neq),
            0x4A => Ok(Opcode::Lt),
            0x4B => Ok(Opcode::Le),
            0x4C => Ok(Opcode::Gt),
            0x4D => Ok(Opcode::Ge),
            0x50 => Ok(Opcode::Not),
            0x51 => Ok(Opcode::And),
            0x52 => Ok(Opcode::Or),
            0x53 => Ok(Opcode::Concat),
            0x58 => Ok(Opcode::BAnd),
            0x59 => Ok(Opcode::BOr),
            0x5A => Ok(Opcode::BXor),
            0x5B => Ok(Opcode::BNot),
            0x5C => Ok(Opcode::Shl),
            0x5D => Ok(Opcode::Shr),
            0x60 => Ok(Opcode::Jmp),
            0x61 => Ok(Opcode::Jz),
            0x62 => Ok(Opcode::Jnz),
            0x63 => Ok(Opcode::Loop),
            0xF0 => Ok(Opcode::Nop),
            0xF1 => Ok(Opcode::Breakpoint),
            0xFF => Ok(Opcode::Halt),
            _ => Err(format!("unknown opcode 0x{byte:02X}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Compiled function
// ---------------------------------------------------------------------------

/// A single compiled function within a bytecode module.
#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeFunction {
    /// Display name (fully qualified).
    pub name: String,
    /// Number of arguments the function expects.
    pub arg_count: u32,
    /// Number of return values.
    pub ret_count: u32,
    /// Number of local variable slots (includes args).
    pub local_count: u32,
    /// Raw bytecode instructions + embedded operands.
    pub code: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Compiled module
// ---------------------------------------------------------------------------

/// A complete compiled NVBC module, ready for execution.
#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeModule {
    /// Constant pool (shared across all functions).
    pub constants: Vec<Value>,
    /// Names of global variables that must be provided at link time.
    pub globals: Vec<String>,
    /// Type table — mirrors the bundle's type list.
    pub types: Vec<String>,
    /// Compiled functions.
    pub functions: Vec<BytecodeFunction>,
    /// Host function descriptors (supplied at run time).
    pub host_functions: Vec<HostFuncDescriptor>,
    /// Index of the entry-point function (0 = main).
    pub entry_point: u32,
}

/// Descriptor for a host function that bridges into native code.
#[derive(Clone, Debug, PartialEq)]
pub struct HostFuncDescriptor {
    pub name: String,
    pub arg_count: u32,
    pub ret_count: u32,
}

// ---------------------------------------------------------------------------
// Code builder
// ---------------------------------------------------------------------------

/// Convenience builder for emitting bytecode.
#[derive(Clone, Debug, Default)]
pub struct CodeBuilder {
    pub code: Vec<u8>,
}

impl CodeBuilder {
    pub fn new() -> Self {
        Self { code: Vec::new() }
    }

    pub fn emit(&mut self, op: Opcode) {
        self.code.push(op as u8);
    }

    pub fn emit_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    pub fn emit_i32(&mut self, v: i32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    pub fn emit_i8(&mut self, v: i8) {
        self.code.push(v as u8);
    }

    pub fn emit_f64(&mut self, v: f64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    pub fn emit_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.emit_u32(bytes.len() as u32);
        self.code.extend_from_slice(bytes);
    }

    /// Returns the current offset (for patching jump targets).
    pub fn offset(&self) -> usize {
        self.code.len()
    }

    /// Patch a previously-written i32 at the given offset.
    pub fn patch_i32(&mut self, offset: usize, v: i32) {
        let bytes = v.to_le_bytes();
        self.code[offset..offset + 4].copy_from_slice(&bytes);
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Serialise a [`BytecodeModule`] into the NVBC binary format.
pub fn encode_module(m: &BytecodeModule) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(NVBC_MAGIC);
    buf.extend_from_slice(&NVBC_VERSION_MAJOR.to_le_bytes());
    buf.extend_from_slice(&NVBC_VERSION_MINOR.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags

    // Constant pool
    buf.extend_from_slice(&(m.constants.len() as u32).to_le_bytes());
    for cv in &m.constants {
        encode_value(&mut buf, cv);
    }

    // Global table
    buf.extend_from_slice(&(m.globals.len() as u32).to_le_bytes());
    for g in &m.globals {
        let b = g.as_bytes();
        buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
        buf.extend_from_slice(b);
    }

    // Type table
    buf.extend_from_slice(&(m.types.len() as u32).to_le_bytes());
    for t in &m.types {
        let b = t.as_bytes();
        buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
        buf.extend_from_slice(b);
    }

    // Function table
    buf.extend_from_slice(&(m.functions.len() as u32).to_le_bytes());
    for f in &m.functions {
        let nb = f.name.as_bytes();
        buf.extend_from_slice(&(nb.len() as u32).to_le_bytes());
        buf.extend_from_slice(nb);
        buf.extend_from_slice(&f.arg_count.to_le_bytes());
        buf.extend_from_slice(&f.ret_count.to_le_bytes());
        buf.extend_from_slice(&f.local_count.to_le_bytes());
        buf.extend_from_slice(&(f.code.len() as u32).to_le_bytes());
        buf.extend_from_slice(&f.code);
    }

    Ok(buf)
}

fn encode_value(buf: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Nil => buf.push(0x00),
        Value::Bool(true) => { buf.push(0x01); buf.push(1); }
        Value::Bool(false) => { buf.push(0x01); buf.push(0); }
        Value::Number(n) => {
            buf.push(0x02);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::String(s) => {
            buf.push(0x03);
            let b = s.as_bytes();
            buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
            buf.extend_from_slice(b);
        }
        Value::Function(fi) => {
            buf.push(0x04);
            buf.extend_from_slice(&fi.to_le_bytes());
        }
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// Deserialise a [`BytecodeModule`] from NVBC binary format.
pub fn decode_module(data: &[u8]) -> Result<BytecodeModule, String> {
    let mut c = Cursor::new(data);

    // Header
    let mut magic = [0u8; 4];
    c.read_exact(&mut magic).map_err(|_| "truncated: no magic")?;
    if &magic != NVBC_MAGIC {
        return Err(format!("invalid magic: {:?}", &magic));
    }
    let _major = read_u16(&mut c)?;
    let _minor = read_u16(&mut c)?;
    let _flags = read_u32(&mut c)?;

    // Constants
    let const_count = read_u32(&mut c)? as usize;
    let mut constants = Vec::with_capacity(const_count);
    for _ in 0..const_count {
        constants.push(decode_value(&mut c)?);
    }

    // Globals
    let global_count = read_u32(&mut c)? as usize;
    let mut globals = Vec::with_capacity(global_count);
    for _ in 0..global_count {
        globals.push(read_string(&mut c)?);
    }

    // Types
    let type_count = read_u32(&mut c)? as usize;
    let mut types = Vec::with_capacity(type_count);
    for _ in 0..type_count {
        types.push(read_string(&mut c)?);
    }

    // Functions
    let func_count = read_u32(&mut c)? as usize;
    let mut functions = Vec::with_capacity(func_count);
    for _ in 0..func_count {
        let name = read_string(&mut c)?;
        let arg_count = read_u32(&mut c)?;
        let ret_count = read_u32(&mut c)?;
        let local_count = read_u32(&mut c)?;
        let code_len = read_u32(&mut c)? as usize;
        let mut code = vec![0u8; code_len];
        c.read_exact(&mut code).map_err(|_| "truncated: code")?;
        functions.push(BytecodeFunction { name, arg_count, ret_count, local_count, code });
    }

    Ok(BytecodeModule {
        constants,
        globals,
        types,
        functions,
        host_functions: Vec::new(),
        entry_point: 0,
    })
}

fn read_u16(c: &mut Cursor<&[u8]>) -> Result<u16, String> {
    let mut b = [0u8; 2];
    c.read_exact(&mut b).map_err(|_| "truncated")?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(c: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut b = [0u8; 4];
    c.read_exact(&mut b).map_err(|_| "truncated")?;
    Ok(u32::from_le_bytes(b))
}

fn read_string(c: &mut Cursor<&[u8]>) -> Result<String, String> {
    let len = read_u32(c)? as usize;
    let mut b = vec![0u8; len];
    c.read_exact(&mut b).map_err(|_| "truncated: string")?;
    String::from_utf8(b).map_err(|e| format!("invalid UTF-8: {e}"))
}

fn decode_value(c: &mut Cursor<&[u8]>) -> Result<Value, String> {
    let tag = read_byte(c)?;
    match tag {
        0x00 => Ok(Value::Nil),
        0x01 => {
            let v = read_byte(c)?;
            Ok(Value::Bool(v != 0))
        }
        0x02 => {
            let mut b = [0u8; 8];
            c.read_exact(&mut b).map_err(|_| "truncated: f64")?;
            Ok(Value::Number(f64::from_le_bytes(b)))
        }
        0x03 => {
            let len = read_u32(c)? as usize;
            let mut b = vec![0u8; len];
            c.read_exact(&mut b).map_err(|_| "truncated: string")?;
            Ok(Value::String(String::from_utf8(b).map_err(|e| format!("invalid utf8: {e}"))?))
        }
        0x04 => {
            let fi = read_u32(c)?;
            Ok(Value::Function(fi))
        }
        _ => Err(format!("unknown constant tag 0x{tag:02X}")),
    }
}

fn read_byte(c: &mut Cursor<&[u8]>) -> Result<u8, String> {
    let mut b = [0u8; 1];
    c.read_exact(&mut b).map_err(|_| "truncated")?;
    Ok(b[0])
}

// ---------------------------------------------------------------------------
// Instruction disassembler (debug)
// ---------------------------------------------------------------------------

/// Disassemble a single instruction at offset `pc` within `code`.
/// Returns `(mnemonic, operands_text, advance_by)`.
pub fn disassemble(code: &[u8], pc: usize) -> Result<(String, String, usize), String> {
    let op = Opcode::try_from(code[pc]).map_err(|_| format!("bad opcode at {pc}"))?;
    let mut off = pc + 1;
    let (mnemonic, operands) = match op {
        Opcode::Nil | Opcode::True | Opcode::False | Opcode::Dup | Opcode::Pop
        | Opcode::NewTable | Opcode::TGet | Opcode::TSet
        | Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div
        | Opcode::Mod | Opcode::Pow | Opcode::Neg
        | Opcode::Eq | Opcode::Neq | Opcode::Lt | Opcode::Le | Opcode::Gt | Opcode::Ge
        | Opcode::Not | Opcode::And | Opcode::Or | Opcode::Concat
        | Opcode::BAnd | Opcode::BOr | Opcode::BXor | Opcode::BNot | Opcode::Shl | Opcode::Shr
        | Opcode::Nop | Opcode::Breakpoint | Opcode::Halt => {
            (format!("{op:?}"), String::new())
        }

        Opcode::Const | Opcode::Load | Opcode::Store
        | Opcode::Closure => {
            let idx = read_u32_at(code, off).ok_or("truncated: u32")?;
            off += 4;
            (format!("const/load/store"), format!("{idx}"))
        }
        Opcode::I8 => {
            let v = *code.get(off).ok_or("truncated: i8")? as i8;
            off += 1;
            (format!("i8"), format!("{v}"))
        }
        Opcode::I32 => {
            let v = read_i32_at(code, off).ok_or("truncated: i32")?;
            off += 4;
            (format!("i32"), format!("{v}"))
        }
        Opcode::F64 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(code.get(off..off+8).ok_or("truncated: f64")?);
            off += 8;
            (format!("f64"), format!("{}", f64::from_le_bytes(b)))
        }
        Opcode::String => {
            let len = read_u32_at(code, off).ok_or("truncated: str len")? as usize;
            off += 4;
            let s = std::str::from_utf8(code.get(off..off+len).ok_or("truncated: str")?)
                .map_err(|e| format!("bad utf8: {e}"))?;
            off += len;
            (format!("string"), format!("\"{s}\""))
        }
        Opcode::Call | Opcode::CallHost => {
            let fi = read_u32_at(code, off).ok_or("truncated: func idx")?;
            off += 4;
            let ac = read_u32_at(code, off).ok_or("truncated: arg count")?;
            off += 4;
            (format!("call"), format!("{fi} {ac}"))
        }
        Opcode::Return => {
            let n = read_u32_at(code, off).ok_or("truncated: ret count")?;
            off += 4;
            (format!("return"), format!("{n}"))
        }
        Opcode::GLoad | Opcode::GStore => {
            let len = read_u32_at(code, off).ok_or("truncated: gname len")? as usize;
            off += 4;
            let s = std::str::from_utf8(code.get(off..off+len).ok_or("truncated: gname")?)
                .map_err(|e| format!("bad utf8: {e}"))?;
            off += len;
            (format!("gload/gstore"), format!("{s}"))
        }
        Opcode::Jmp | Opcode::Jz | Opcode::Jnz | Opcode::Loop => {
            let v = read_i32_at(code, off).ok_or("truncated: offset")?;
            off += 4;
            let target = (pc as i32 + v) as usize;
            (format!("jmp/jz/jnz"), format!("{v} -> {target}"))
        }
    };
    Ok((mnemonic, operands, off - pc))
}

fn read_u32_at(code: &[u8], off: usize) -> Option<u32> {
    let b = code.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32_at(code: &[u8], off: usize) -> Option<i32> {
    let b = code.get(off..off + 4)?;
    Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Pretty-print a full bytecode module's disassembly.
pub fn disassemble_module(m: &BytecodeModule) -> String {
    let mut out = String::new();
    out.push_str(&format!(";; NVBC module: {} constants, {} functions, {} globals, {} types\n",
        m.constants.len(), m.functions.len(), m.globals.len(), m.types.len()));
    for (i, f) in m.functions.iter().enumerate() {
        out.push_str(&format!("\n;; function [{}] {} (args={}, ret={}, locals={})\n",
            i, f.name, f.arg_count, f.ret_count, f.local_count));
        let mut pc = 0;
        while pc < f.code.len() {
            match disassemble(&f.code, pc) {
                Ok((mnem, ops, adv)) => {
                    if ops.is_empty() {
                        out.push_str(&format!("  {:04x}  {mnem}\n", pc));
                    } else {
                        out.push_str(&format!("  {:04x}  {mnem} {}\n", pc, ops));
                    }
                    pc += adv;
                }
                Err(e) => {
                    out.push_str(&format!("  {:04x}  !! {e}\n", pc));
                    pc += 1;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty_module() {
        let m = BytecodeModule {
            constants: vec![],
            globals: vec![],
            types: vec![],
            functions: vec![],
            host_functions: vec![],
            entry_point: 0,
        };
        let bytes = encode_module(&m).unwrap();
        let decoded = decode_module(&bytes).unwrap();
        assert_eq!(decoded.constants.len(), 0);
        assert_eq!(decoded.functions.len(), 0);
    }

    #[test]
    fn round_trip_with_data() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);

        let m = BytecodeModule {
            constants: vec![Value::Number(42.0)],
            globals: vec!["OUTPUT".to_string()],
            types: vec!["int".to_string()],
            functions: vec![BytecodeFunction {
                name: "main".into(),
                arg_count: 0,
                ret_count: 1,
                local_count: 0,
                code: cb.code,
            }],
            host_functions: vec![],
            entry_point: 0,
        };
        let bytes = encode_module(&m).unwrap();
        let decoded = decode_module(&bytes).unwrap();
        assert_eq!(decoded.constants.len(), 1);
        assert_eq!(decoded.globals, vec!["OUTPUT"]);
        assert_eq!(decoded.types, vec!["int"]);
        assert_eq!(decoded.functions.len(), 1);
        assert_eq!(decoded.functions[0].name, "main");
        assert_eq!(decoded.functions[0].code.len(), 10); // CONST+u32 + RETURN+u32 = 2×(1+4)
    }

    #[test]
    fn invalid_magic_fails() {
        assert!(decode_module(b"XXXX").is_err());
    }

    #[test]
    fn disassemble_works() {
        let mut cb = CodeBuilder::new();
        cb.emit(Opcode::Const);
        cb.emit_u32(0);
        cb.emit(Opcode::Return);
        cb.emit_u32(1);

        let m = BytecodeModule {
            constants: vec![Value::Number(1.0)],
            globals: vec![],
            types: vec![],
            functions: vec![BytecodeFunction {
                name: "test".into(),
                arg_count: 0,
                ret_count: 1,
                local_count: 0,
                code: cb.code,
            }],
            host_functions: vec![],
            entry_point: 0,
        };
        let text = disassemble_module(&m);
        assert!(text.contains("test"));
        assert!(text.contains("return"));
    }
}
