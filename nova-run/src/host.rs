//! Host bundle system — define native functions and types for the VM.
//!
//! A "host bundle" is a set of native functions registered with the VM
//! that appear as a synthetic Nova bundle.  Host bundles are the bridge
//! between the VM and the outside world (I/O, FFI, etc.).
//!
//! ## Config format (TOML)
//!
//! ```toml
//! [bundle]
//! name = "std"
//!
//! [types]
//! "std.IO" = "opaque"
//! "std.String" = "string"
//!
//! [functions.io_print]
//! args = ["std.String"]
//! rets = []
//! native = "io_print"
//! ```

use std::collections::HashMap;

/// A host function implementation: takes a slice of `Value`s, returns
/// a vector of result values, or an error string.
pub type HostFn = fn(&[crate::bytecode::Value]) -> Result<Vec<crate::bytecode::Value>, String>;

/// Describes a single function exported by a host bundle.
#[derive(Clone, Debug)]
pub struct HostFuncDef {
    /// Fully qualified name.
    pub name: String,
    pub arg_count: u32,
    pub ret_count: u32,
    /// Pointer to the native implementation.
    pub native: HostFn,
}

/// A complete synthetic bundle definition.
#[derive(Clone, Debug)]
pub struct HostBundle {
    pub name: String,
    /// FQN → type kind (e.g. "opaque", "string", "number").
    pub types: HashMap<String, String>,
    pub functions: Vec<HostFuncDef>,
}

impl HostBundle {
    /// Build a simple "stdio" host bundle with print and read.
    pub fn stdio() -> Self {
        HostBundle {
            name: "std".into(),
            types: HashMap::from([
                ("std.IO".into(), "opaque".into()),
                ("std.String".into(), "string".into()),
            ]),
            functions: vec![
                HostFuncDef {
                    name: "std.print".into(),
                    arg_count: 1,
                    ret_count: 0,
                    native: host_print,
                },
                HostFuncDef {
                    name: "std.println".into(),
                    arg_count: 1,
                    ret_count: 0,
                    native: host_println,
                },
            ],
        }
    }
}

// ── Built-in host functions ───────────────────────────────────────────

fn host_print(args: &[crate::bytecode::Value]) -> Result<Vec<crate::bytecode::Value>, String> {
    if let Some(v) = args.first() {
        print!("{}", format_value(v));
    }
    Ok(vec![])
}

fn host_println(args: &[crate::bytecode::Value]) -> Result<Vec<crate::bytecode::Value>, String> {
    if let Some(v) = args.first() {
        println!("{}", format_value(v));
    } else {
        println!();
    }
    Ok(vec![])
}

/// Format a [`Value`](crate::bytecode::Value) for display.
pub fn format_value(v: &crate::bytecode::Value) -> String {
    match v {
        crate::bytecode::Value::Nil => "nil".into(),
        crate::bytecode::Value::Bool(b) => b.to_string(),
        crate::bytecode::Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        crate::bytecode::Value::String(s) => s.clone(),
        crate::bytecode::Value::Function(_) => "<function>".into(),
    }
}
