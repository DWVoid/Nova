//! CLI argument parsing for `nvrun`.

use clap::Parser;

/// Nova bytecode runner — load, link, compile, and execute Nova bundles.
#[derive(Parser, Debug)]
#[command(name = "nvrun", author, version = "0.1.0", about)]
pub struct Args {
    /// Path to the main NVIL bundle file to execute.
    pub bundle: String,

    /// Fully qualified function name to call (default: `{bundle_ns}.main`).
    #[arg(short = 'e', long)]
    pub entry: Option<String>,

    /// Directories to search for dependency bundles.
    #[arg(short = 'L', long = "lib", default_value = "lib")]
    pub lib_paths: Vec<String>,

    /// Path to a host bundle TOML configuration file.
    #[arg(short = 'H', long = "host")]
    pub host_config: Option<String>,

    /// Print bytecode disassembly instead of executing.
    #[arg(short = 'd', long)]
    pub disassemble: bool,
}

impl Args {
    pub fn parse_or_exit() -> Self {
        Self::parse()
    }
}
