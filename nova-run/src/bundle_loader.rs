//! Load NVIL bundle files from disk and parse into [`Bundle`](nova_analyze::bundle::Bundle).

use std::path::Path;
use nova_analyze::bundle::decode::decode_bundle;

/// Find and load a single NVIL bundle by name.
///
/// Searches `lib_paths` for `<name>.nvb` (i.e., the canonically named
/// compiled bundle).  Returns the loaded [`Bundle`] and the file path it was
/// found at.
pub fn load_bundle(name: &str, lib_paths: &[impl AsRef<Path>]) -> Result<LoadedBundle, LoadError> {
    for dir in lib_paths {
        let path = dir.as_ref().join(format!("{name}.nvb"));
        if path.is_file() {
            return load_bundle_file(&path);
        }
    }
    Err(LoadError::NotFound(name.to_string()))
}

/// Load a bundle from a specific file path.
pub fn load_bundle_file(path: impl AsRef<Path>) -> Result<LoadedBundle, LoadError> {
    let path = path.as_ref();
    let data = std::fs::read(path)
        .map_err(|e| LoadError::Io(path.to_path_buf(), e.to_string()))?;
    let bundle = decode_bundle(&data)
        .map_err(|e| LoadError::Decode(path.to_path_buf(), e))?;
    Ok(LoadedBundle { path: path.to_path_buf(), bundle })
}

/// A bundle that has been loaded into memory.
#[derive(Clone, Debug)]
pub struct LoadedBundle {
    pub path: std::path::PathBuf,
    pub bundle: nova_analyze::bundle::Bundle,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LoadError {
    NotFound(String),
    Io(std::path::PathBuf, String),
    Decode(std::path::PathBuf, String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound(name) => write!(f, "bundle '{name}' not found"),
            LoadError::Io(p, e) => write!(f, "I/O error reading '{}': {e}", p.display()),
            LoadError::Decode(p, e) => write!(f, "decode error in '{}': {e}", p.display()),
        }
    }
}

impl std::error::Error for LoadError {}
