use nova::semantic::{analyze_bundle_with_linking, Bundle, SemanticModel, SemanticDiagnostic};
use nova::semantic::bundle::{BundleName, Version, BundleMetadata, NamespaceExports, BundleDependency, VersionConstraint, DependencyVisibility};
use nova::lexical::lexer::Lexer;
use nova::syntax::parser::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Example bundle configuration file format (bundle.toml)
#[derive(Serialize, Deserialize, Debug)]
pub struct BundleConfig {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub source_files: Vec<String>,
    pub dependencies: Option<Vec<DependencyConfig>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DependencyConfig {
    pub name: String,
    pub version: String,
    pub path: Option<String>,  // Local path dependency
    pub git: Option<String>,   // Git repository dependency  
    pub optional: Option<bool>,
}

/// Bundle dependency resolver and project manager
pub struct BundleManager {
    /// Cache of resolved bundles
    bundle_cache: HashMap<(String, String), Bundle>,
    /// Base directory for relative path resolution
    base_directory: PathBuf,
    /// Verbose output
    verbose: bool,
}

impl BundleManager {
    pub fn new(base_directory: impl AsRef<Path>) -> Self {
        Self {
            bundle_cache: HashMap::new(),
            base_directory: base_directory.as_ref().to_path_buf(),
            verbose: false,
        }
    }

    pub fn with_verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Load and analyze a bundle project from a configuration file
    pub fn analyze_bundle_project(&mut self, config_path: impl AsRef<Path>) -> Result<SemanticModel, Box<dyn std::error::Error>> {
        let config_path = config_path.as_ref();
        
        if self.verbose {
            println!("📋 Loading bundle configuration from {:?}", config_path);
        }

        // Load bundle configuration
        let config_content = fs::read_to_string(config_path)?;
        let config: BundleConfig = toml::from_str(&config_content)?;

        if self.verbose {
            println!("📦 Bundle: {} v{}", config.name, config.version);
            if let Some(deps) = &config.dependencies {
                println!("   Dependencies: {}", deps.len());
                for dep in deps {
                    println!("     - {} v{}", dep.name, dep.version);
                }
            }
        }

        // Set base directory relative to config file
        if let Some(parent) = config_path.parent() {
            self.base_directory = parent.to_path_buf();
        }

        // Resolve dependencies first
        let dependencies = if let Some(deps) = &config.dependencies {
            self.resolve_dependencies(deps)?
        } else {
            Vec::new()
        };

        // Parse main bundle source files
        let main_bundle = self.create_bundle_from_config(&config)?;

        // Perform analysis with dependencies
        match analyze_bundle_with_linking(main_bundle.compilation_units, dependencies) {
            Ok(model) => Ok(model),
            Err(diagnostics) => {
                eprintln!("❌ Semantic analysis failed:");
                for diagnostic in &diagnostics {
                    eprintln!("   {}: {}", diagnostic.category, diagnostic.message);
                }
                Err(format!("Analysis failed with {} errors", diagnostics.len()).into())
            }
        }
    }

    /// Resolve all dependencies for a bundle
    fn resolve_dependencies(&mut self, dep_configs: &[DependencyConfig]) -> Result<Vec<Bundle>, Box<dyn std::error::Error>> {
        let mut resolved_bundles = Vec::new();

        for dep_config in dep_configs {
            if self.verbose {
                println!("🔍 Resolving dependency: {} v{}", dep_config.name, dep_config.version);
            }

            let cache_key = (dep_config.name.clone(), dep_config.version.clone());
            
            // Check cache first
            if let Some(cached_bundle) = self.bundle_cache.get(&cache_key) {
                if self.verbose {
                    println!("   ✅ Found in cache");
                }
                resolved_bundles.push(cached_bundle.clone());
                continue;
            }

            // Resolve the dependency
            let bundle = self.resolve_single_dependency(dep_config)?;
            
            // Cache the resolved bundle
            self.bundle_cache.insert(cache_key, bundle.clone());
            resolved_bundles.push(bundle);
        }

        Ok(resolved_bundles)
    }

    /// Resolve a single dependency
    fn resolve_single_dependency(&mut self, dep_config: &DependencyConfig) -> Result<Bundle, Box<dyn std::error::Error>> {
        match (&dep_config.path, &dep_config.git) {
            // Local path dependency
            (Some(path), None) => {
                if self.verbose {
                    println!("   📁 Local path dependency: {}", path);
                }
                self.load_local_dependency(path, dep_config)
            }
            
            // Git dependency (simplified - would need actual git operations)
            (None, Some(git_url)) => {
                if self.verbose {
                    println!("   🌐 Git dependency: {}", git_url);
                }
                // In a real implementation, this would:
                // 1. Clone or fetch the git repository
                // 2. Check out the appropriate version/tag
                // 3. Load the bundle from the checked out code
                Err(format!("Git dependencies not yet implemented: {}", git_url).into())
            }
            
            // Registry dependency (would fetch from package registry)
            (None, None) => {
                if self.verbose {
                    println!("   🏪 Registry dependency (not implemented)");
                }
                Err(format!("Registry dependencies not yet implemented: {} v{}", 
                    dep_config.name, dep_config.version).into())
            }
            
            // Both path and git specified - ambiguous
            (Some(_), Some(_)) => {
                Err("Dependency cannot have both 'path' and 'git' specified".into())
            }
        }
    }

    /// Load a local path dependency
    fn load_local_dependency(&mut self, path: &str, dep_config: &DependencyConfig) -> Result<Bundle, Box<dyn std::error::Error>> {
        let dep_path = self.base_directory.join(path);
        let config_path = dep_path.join("bundle.toml");

        if !config_path.exists() {
            return Err(format!("Dependency configuration not found: {:?}", config_path).into());
        }

        // Load dependency configuration
        let config_content = fs::read_to_string(&config_path)?;
        let dep_bundle_config: BundleConfig = toml::from_str(&config_content)?;

        // Verify name and version match
        if dep_bundle_config.name != dep_config.name {
            return Err(format!(
                "Dependency name mismatch: expected '{}', found '{}' in {:?}",
                dep_config.name, dep_bundle_config.name, config_path
            ).into());
        }

        // Create bundle with proper base directory
        let original_base = self.base_directory.clone();
        self.base_directory = dep_path;
        
        let bundle = self.create_bundle_from_config(&dep_bundle_config);
        
        self.base_directory = original_base;
        
        bundle
    }

    /// Create a Bundle from a BundleConfig
    fn create_bundle_from_config(&self, config: &BundleConfig) -> Result<Bundle, Box<dyn std::error::Error>> {
        let mut compilation_units = Vec::new();

        // Parse all source files
        for source_file in &config.source_files {
            let file_path = self.base_directory.join(source_file);
            
            if self.verbose {
                println!("   📄 Processing source file: {:?}", file_path);
            }

            let source_code = fs::read_to_string(&file_path)
                .map_err(|e| format!("Failed to read {:?}: {}", file_path, e))?;

            let tokens = Lexer::new(&source_code).lex_all()
                .map_err(|e| format!("Lexer error in {:?}: {}", file_path, e.message))?;

            let chunk = Parser::new(tokens).parse_chunk()
                .map_err(|e| format!("Parser error in {:?}: {}", file_path, e.message))?;

            compilation_units.push(chunk);
        }

        // Parse version
        let version = Version {
            major: 1,  // Simplified version parsing
            minor: 0,
            patch: 0,
            pre_release: None,
        };

        // Create dependencies list
        let dependencies = if let Some(deps) = &config.dependencies {
            deps.iter().map(|dep| BundleDependency {
                name: BundleName::from(&dep.name),
                version_constraint: VersionConstraint::Compatible(Version {
                    major: 1, minor: 0, patch: 0, pre_release: None,
                }),
                visibility: if dep.optional.unwrap_or(false) {
                    DependencyVisibility::Private
                } else {
                    DependencyVisibility::Public
                },
            }).collect()
        } else {
            Vec::new()
        };

        Ok(Bundle {
            name: BundleName::from(&config.name),
            version,
            compilation_units,
            dependencies,
            exports: NamespaceExports {
                exported_definitions: HashMap::new(),
            },
            metadata: BundleMetadata {
                description: config.description.clone(),
                authors: config.authors.clone().unwrap_or_default(),
                ..Default::default()
            },
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() != 2 {
        eprintln!("Usage: {} <bundle.toml>", args[0]);
        eprintln!("Example: {} examples/sample-project/bundle.toml", args[0]);
        std::process::exit(1);
    }

    let config_path = &args[1];
    let mut manager = BundleManager::new(std::env::current_dir()?)
        .with_verbose();

    match manager.analyze_bundle_project(config_path) {
        Ok(model) => {
            println!("🎉 Bundle analysis completed successfully!");
            println!("Bundle: {} v{}", model.bundle.name, model.bundle.version);
            println!("Namespaces: {}", model.namespace_tree.namespaces.len());
            println!("Exported symbols: {}", model.symbol_table.exported_symbols.len());
            
            // Print comprehensive report if requested
            crate::print_semantic_analysis_report(&model);
            
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Bundle analysis failed: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_bundle_config_parsing() {
        let config_toml = r#"
name = "test-bundle"
version = "1.0.0"
description = "A test bundle"
authors = ["Test Author"]
source_files = ["src/main.nova", "src/utils.nova"]

[[dependencies]]
name = "std"
version = "1.0.0"
path = "../stdlib"

[[dependencies]]
name = "math"
version = "0.5.0"
optional = true
        "#;

        let config: BundleConfig = toml::from_str(config_toml).unwrap();
        
        assert_eq!(config.name, "test-bundle");
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.source_files.len(), 2);
        assert!(config.dependencies.is_some());
        
        let deps = config.dependencies.unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "std");
        assert_eq!(deps[1].optional, Some(true));
    }

    #[test] 
    fn test_bundle_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = BundleManager::new(temp_dir.path());
        
        assert_eq!(manager.base_directory, temp_dir.path());
        assert!(manager.bundle_cache.is_empty());
        assert!(!manager.verbose);
    }
}