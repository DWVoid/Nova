//! `nvc` – the Nova standalone compiler driver.
//!
//! # Invocation
//!
//! ```text
//! nvc [--project <path>]
//! ```
//!
//! # Pipeline
//!
//! ```text
//! bundle.toml
//!     │  read source_files list
//!     ▼
//! stat_source_files()          – real fs::metadata for each file
//!     │
//!     ▼
//! SemanticSession::open()      – backed by FileSystemStorage + RealFileAccess
//!     │  auto-detects warm/cold start
//!     ▼
//! session.update_files(stats)  – register / update input nodes
//!     │
//!     ▼
//! session.checkpoint()         – begin a checkpoint
//!     │
//!     ▼
//! session.run().await          – incremental propagation (values flush as they compute)
//!     │
//!     ▼
//! session.commit().await       – persist graph topology + all values atomically
//!     │
//!     ▼
//! report printed to stdout
//! ```

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
// CLI argument parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Args {
    project: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut project = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" | "-p" => {
                project = Some(PathBuf::from(args.next().unwrap_or_else(|| {
                    eprintln!("error: --project requires a path argument");
                    process::exit(1);
                })));
            }
            "--help" | "-h" => { print_usage(); process::exit(0); }
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
    eprintln!("  -h, --help             Print this help message");
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = parse_args();

    // 1. Locate bundle.toml
    let manifest_path = args.project.unwrap_or_else(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("error: cannot determine current directory: {e}");
            process::exit(1);
        });
        find_manifest(&cwd).unwrap_or_else(|| {
            eprintln!("error: no bundle.toml found in '{}' or any parent directory", cwd.display());
            process::exit(1);
        })
    });

    // 2. Parse project manifest
    let manifest = load_manifest(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    println!("nvc: compiling {} v{} ({})",
        manifest.name, manifest.version, manifest.root.display());
    if !manifest.description.is_empty() { println!("     {}", manifest.description); }
    println!("     {} source file(s)", manifest.source_files.len());

    // 3. Set up incremental storage
    let storage = Arc::new(FileSystemStorage::new(&manifest.incremental_dir));
    storage.ensure_dir().await.unwrap_or_else(|e| {
        eprintln!("error: {e}"); process::exit(1);
    });
    println!("     incremental cache: {}", manifest.incremental_dir.display());

    // 4. Stat source files
    let stats = stat_source_files(&manifest).unwrap_or_else(|e| {
        eprintln!("error: {e}"); process::exit(1);
    });

    // 5. Open the semantic session (auto-detects warm/cold start)
    let fs = Arc::new(RealFileAccess);
    let storage_arc: Arc<dyn nova_incremental::Storage> = Arc::clone(&storage) as _;
    let fs_arc: Arc<dyn nova_analyze::semantic::file_access::FileAccess> = Arc::clone(&fs) as _;

    let session = SemanticSession::open(storage_arc, fs_arc).await.unwrap_or_else(|e| {
        eprintln!("error: failed to open semantic session: {e}");
        process::exit(1);
    });

    // 6. Update file set
    session.set_files(stats).unwrap_or_else(|e| {
        eprintln!("error: failed to register source files: {e:?}");
        process::exit(1);
    });

    // 7. Checkpoint + run + commit
    session.checkpoint().await.unwrap_or_else(|e| {
        eprintln!("error: failed to begin checkpoint: {e}");
        process::exit(1);
    });

    println!("nvc: running incremental update …");
    let report = session.run().await;

    // 8. Always commit (preserves partial results for next run)
    session.commit().await.unwrap_or_else(|e| {
        eprintln!("warning: failed to commit incremental state: {e}");
    });

    // 9. Print report
    println!("nvc: update complete");
    println!("     transforms evaluated : {}", report.transforms_evaluated);
    println!("     transforms changed   : {}", report.transforms_changed);
    println!("     transforms skipped   : {}", report.transforms_skipped);
    println!("     transforms blocked   : {}", report.transforms_blocked);

    if !report.errors.is_empty() {
        eprintln!("nvc: {} error(s) during update:", report.errors.len());
        for (node_id, err) in &report.errors {
            eprintln!("     - {node_id}: {err:?}");
        }
        process::exit(1);
    }
}