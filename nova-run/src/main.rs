//! `nvrun` – the Nova bytecode runner.
//!
//! Loads an NVIL bundle, links it against dependency bundles found in
//! configured library paths, compiles the result to NVBC, resolves host
//! function bindings, and executes the entry-point function.
//!
//! # Usage
//!
//! ```text
//! nvrun <bundle.nvb> [-e <entry>] [-L <lib_dir>] [-H <host_config>] [-d]
//! ```
//!
//! # Pipeline
//!
//! ```text
//! bundle.nvb         – load NVIL bundle from disk
//!   deps/*.nvb       – load dependency bundles from lib dirs
//!       │
//!       ▼
//!   [linker]         – merge type/func/const tables, remap indices
//!       │
//!       ▼
//!   [compiler]       – compile to NVBC bytecode
//!       │
//!       ▼
//!   [disassemble]    – (optional) print bytecode and exit
//!       │
//!       ▼
//!   [VM]             – execute the entry function
//!       │
//!       ▼
//!   result           – print return values
//! ```

mod bundle_loader;
mod bytecode;
mod cli;
mod compiler;
mod host;
mod linker;
mod vm;

use std::process;
use crate::bundle_loader::{load_bundle, load_bundle_file};
use crate::cli::Args;
use crate::compiler::compile;
use crate::host::HostBundle;
use crate::linker::link;
use crate::vm::Vm;

fn main() {
    let args = Args::parse_or_exit();

    // 1. Load the main bundle
    let main = load_bundle_file(&args.bundle).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    println!("nvrun: loaded main bundle '{}'", main.path.display());

    // 2. Load dependency bundles
    let mut deps = Vec::new();
    for dep_name in find_dependencies(&main.bundle) {
        match load_bundle(&dep_name, &args.lib_paths) {
            Ok(lb) => {
                println!("nvrun: loaded dependency '{}'", lb.path.display());
                deps.push(lb.bundle);
            }
            Err(e) => {
                eprintln!("warning: dependency '{dep_name}' not found: {e}");
            }
        }
    }

    // 3. Load host bundles
    let host_bundles = load_host_bundles(&args.host_config);

    // 4. Link bundles together
    // We need to make the host types/functions available to the linker.
    // For now, the linker only processes NL bundles, not synthetic host bundles.
    let program = link(main.bundle, deps);

    // 5. Collect host function descriptors
    let host_funcs: Vec<_> = host_bundles.iter()
        .flat_map(|hb| hb.functions.iter())
        .cloned()
        .collect();

    // 6. Determine entry point
    let entry_name = args.entry.unwrap_or_else(|| {
        // Default: program.main
        format!("{}.main", program.name)
    });

    // 7. Compile to bytecode
    let compiled = compile(&program, &entry_name, &host_funcs);

    // 8. Optionally print the compiled bytecode disassembly and exit.
    if args.disassemble {
        println!("{}", crate::bytecode::disassemble_module(&compiled.module));
        return;
    }

    // 9. Extract host function implementations
    let host_fn_ptrs: Vec<_> = host_bundles.iter()
        .flat_map(|hb| hb.functions.iter())
        .map(|f| f.clone())
        .collect();

    // 10. Run the VM
    let entry_point = compiled.module.entry_point;
    let entry = compiled.module.entry_point;
    let mut vm = Vm::new(compiled.module, host_fn_ptrs);
    println!("nvrun: executing '{}' …", vm.entry_name(entry));
    match vm.run(entry_point) {
        Ok(results) => {
            if results.is_empty() {
                println!("nvrun: (no return value)");
            } else {
                for (i, v) in results.iter().enumerate() {
                    println!("nvrun: result[{}] = {}", i, host::format_value(v));
                }
            }
        }
        Err(e) => {
            eprintln!("nvrun: runtime error: {e}");
            process::exit(1);
        }
    }
}

/// Extract dependency bundle names from the main bundle's metadata
/// or dependency section.
fn find_dependencies(bundle: &nova_analyze::bundle::Bundle) -> Vec<String> {
    bundle.dependencies.iter()
        .map(|d| d.id.clone())
        .collect()
}

/// Load host bundles from config file or use built-in defaults.
fn load_host_bundles(config_path: &Option<String>) -> Vec<HostBundle> {
    let mut bundles = Vec::new();

    // Always include stdio
    bundles.push(HostBundle::stdio());

    // If a config file is provided, parse additional host bundles
    if let Some(_path) = config_path {
        // TODO: parse TOML host config
        eprintln!("warning: host config files not yet supported, using built-ins only");
    }

    bundles
}
