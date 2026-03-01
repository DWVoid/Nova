//! `nvc` – the Nova standalone compiler driver.
//!
//! # Invocation
//!
//! ```text
//! nvc [--project <path>]
//! ```
//!
//! | Flag | Default | Meaning |
//! |------|---------|---------|
//! | `--project <path>` | current directory | Path to `bundle.toml` **or** the directory that contains it. |
//!
//! If `--project` is not supplied the driver searches upward from the current
//! working directory for a `bundle.toml`, mirroring how `cargo` finds
//! `Cargo.toml`.
//!
//! # Pipeline (current)
//!
//! ```text
//! bundle.toml
//!     │  read source_files list
//!     ▼
//! stat_source_files()          – real fs::metadata for each file
//!     │
//!     ▼
//! SemanticSession::new()       – backed by FileSystemStorage (loose files)
//!     │                          and RealFileAccess
//!     ▼
//! session.update_files(stats)  – register / update input nodes
//!     │
//!     ▼
//! session.run().await          – incremental propagation
//!     │
//!     ▼
//! report printed to stdout
//! ```
//!
//! The incremental state is persisted under
//! `<project>/target/nova-incremental/` so that a second run of `nvc` on an
//! unchanged project does zero recomputation.
//!
//! # Future work
//!
//! Once the semantic, type-checking, and code-generation stages are
//! implemented, `session.run()` will return richer diagnostics and the driver
//! will emit object files / bytecode into `<project>/target/`.

mod file_storage;
mod project;
mod real_fs;

use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use file_storage::FileSystemStorage;
use project::{find_manifest, load_manifest, stat_source_files};
use real_fs::RealFileAccess;

use nova_analyze::semantic::SemanticSession;

// ---------------------------------------------------------------------------
// CLI argument parsing (hand-rolled, no external dep needed yet)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Args {
    /// Path to `bundle.toml` or the directory containing it.
    project: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1); // skip the binary name
    let mut project = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" | "-p" => {
                project = Some(PathBuf::from(
                    args.next().unwrap_or_else(|| {
                        eprintln!("error: --project requires a path argument");
                        process::exit(1);
                    }),
                ));
            }
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("error: unknown argument '{other}'");
                print_usage();
                process::exit(1);
            }
        }
    }

    Args { project }
}

fn print_usage() {
    eprintln!("Usage: nvc [--project <path>]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -p, --project <path>   Path to bundle.toml or the project directory");
    eprintln!("                         (default: search upward from cwd)");
    eprintln!("  -h, --help             Print this help message");
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = parse_args();

    // ── 1. Locate bundle.toml ─────────────────────────────────────────────
    let manifest_path = args.project.unwrap_or_else(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("error: cannot determine current directory: {e}");
            process::exit(1);
        });
        find_manifest(&cwd).unwrap_or_else(|| {
            eprintln!(
                "error: no bundle.toml found in '{}' or any parent directory",
                cwd.display()
            );
            process::exit(1);
        })
    });

    // ── 2. Parse project manifest ─────────────────────────────────────────
    let manifest = load_manifest(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    println!(
        "nvc: compiling {} v{} ({})",
        manifest.name,
        manifest.version,
        manifest.root.display()
    );
    if !manifest.description.is_empty() {
        println!("     {}", manifest.description);
    }
    println!("     {} source file(s)", manifest.source_files.len());

    // ── 3. Set up incremental storage under <project>/target/nova-incremental/
    let storage = Arc::new(FileSystemStorage::new(&manifest.incremental_dir));
    storage.ensure_dir().await.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    println!(
        "     incremental cache: {}",
        manifest.incremental_dir.display()
    );

    // ── 4. Stat every source file ─────────────────────────────────────────
    let stats = stat_source_files(&manifest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    // ── 5. Build the semantic session ─────────────────────────────────────
    let fs = Arc::new(RealFileAccess);
    let mut session = SemanticSession::new(storage, fs);

    // Register all source files as inputs.  New files are added; files whose
    // stat matches the persisted value are skipped by the incremental engine.
    session.update_files(stats).unwrap_or_else(|e| {
        eprintln!("error: failed to register source files: {e:?}");
        process::exit(1);
    });

    // ── 6. Run the incremental update ─────────────────────────────────────
    println!("nvc: running incremental update …");
    let report = session.run().await;

    // ── 7. Print report ───────────────────────────────────────────────────
    println!("nvc: update complete");
    println!("     nodes evaluated  : {}", report.nodes_evaluated);
    println!("     nodes changed    : {}", report.nodes_changed);
    println!("     nodes skipped    : {}", report.nodes_skipped);
    println!("     nodes blocked    : {}", report.nodes_blocked);
    if !report.errors.is_empty() {
        eprintln!("nvc: {} error(s) during update:", report.errors.len());
        for (node_id, err) in &report.errors {
            eprintln!("     - {node_id}: {err:?}");
        }
        process::exit(1);
    }
}