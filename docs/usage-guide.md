# Nova Semantic Analysis System Usage Guide

This document explains how to use Nova's complete semantic analysis system for processing source files and handling bundle dependencies.

## Basic Usage - Single Bundle Analysis

### Simple API Usage

For basic single-bundle analysis (most common use case):

```rust
use nova::semantic::analyze_bundle;
use nova::lexical::lexer::Lexer;
use nova::syntax::parser::Parser;

// Process a single Nova source file
fn analyze_source_file(source_code: &str) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    // 1. Lexical Analysis
    let tokens = Lexer::new(source_code).lex_all()?;
    
    // 2. Syntax Parsing  
    let chunk = Parser::new(tokens).parse_chunk()?;
    
    // 3. Complete Semantic Analysis (all 9 phases)
    let semantic_model = analyze_bundle(vec![chunk])?;
    
    Ok(semantic_model)
}

// Example usage
fn main() {
    let source = r#"
    namespace MyApp.Utils;
    
    @doc("A utility function for adding numbers")
    export define add(x: integer, y: integer): integer
        return x + y
    end
    
    @test("Addition test")
    define test_add(): unit
        assert(add(2, 3) == 5)
    end
    "#;
    
    match analyze_source_file(source) {
        Ok(model) => {
            println!("✅ Analysis successful!");
            println!("Bundle: {}", model.bundle.name);
            println!("Namespaces: {}", model.namespace_tree.namespaces.len());
            println!("Exports: {}", model.symbol_table.exported_symbols.len());
        }
        Err(diagnostics) => {
            println!("❌ Analysis failed with {} errors:", diagnostics.len());
            for diagnostic in diagnostics {
                println!("  {}: {}", diagnostic.category, diagnostic.message);
            }
        }
    }
}
```

### Processing Multiple Source Files

For multi-file bundles:

```rust
use std::fs;
use std::path::Path;

fn analyze_bundle_from_files(file_paths: &[&str]) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    let mut chunks = Vec::new();
    
    // Process each source file
    for file_path in file_paths {
        let source_code = fs::read_to_string(file_path)
            .map_err(|e| vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Failed to read file {}: {}", file_path, e),
                location: Position::start(),
                category: DiagnosticCategory::SymbolResolution,
            }])?;
        
        let tokens = Lexer::new(&source_code).lex_all()?;
        let chunk = Parser::new(tokens).parse_chunk()?;
        chunks.push(chunk);
    }
    
    // Analyze all compilation units together
    analyze_bundle(chunks)
}

// Example: analyze a complete bundle
fn main() {
    let source_files = &[
        "src/main.nova",
        "src/utils.nova", 
        "src/models.nova"
    ];
    
    match analyze_bundle_from_files(source_files) {
        Ok(model) => println!("Bundle analysis complete!"),
        Err(diagnostics) => println!("Analysis failed: {} errors", diagnostics.len()),
    }
}
```

## Advanced Usage - Bundle Dependencies

### Dependency Bundle Resolution

Nova's semantic analysis system supports dependency resolution through the cross-bundle linking system:

```rust
use nova::semantic::{analyze_bundle_with_linking, Bundle};

fn analyze_with_dependencies(
    main_source_files: &[&str],
    dependency_bundles: Vec<Bundle>
) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    
    // Parse main bundle source files
    let mut chunks = Vec::new();
    for file_path in main_source_files {
        let source_code = fs::read_to_string(file_path)?;
        let tokens = Lexer::new(&source_code).lex_all()?;
        let chunk = Parser::new(tokens).parse_chunk()?;
        chunks.push(chunk);
    }
    
    // Use the linking-aware analysis
    analyze_bundle_with_linking(chunks, dependency_bundles)
}
```

### Bundle Creation and Management

To create dependency bundles:

```rust
use nova::semantic::bundle::{Bundle, BundleMetadata, Version, BundleDependency};

fn create_dependency_bundle(
    name: &str,
    version: &str,
    source_files: &[&str]
) -> Result<Bundle, Box<dyn std::error::Error>> {
    
    // Parse source files into compilation units
    let mut compilation_units = Vec::new();
    for file_path in source_files {
        let source_code = fs::read_to_string(file_path)?;
        let tokens = Lexer::new(&source_code).lex_all()?;
        let chunk = Parser::new(tokens).parse_chunk()?;
        compilation_units.push(chunk);
    }
    
    // Create bundle structure
    let bundle = Bundle {
        name: BundleName::from(name),
        version: Version::parse(version)?,
        compilation_units,
        dependencies: Vec::new(), // Dependencies of this bundle
        exports: NamespaceExports {
            exported_definitions: HashMap::new(), // Will be populated during analysis
        },
        metadata: BundleMetadata::default(),
    };
    
    Ok(bundle)
}

// Example: Create a complete project with dependencies
fn analyze_project_with_deps() -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    // 1. Create dependency bundles
    let std_lib = create_dependency_bundle(
        "std",
        "1.0.0", 
        &["stdlib/collections.nova", "stdlib/io.nova"]
    )?;
    
    let math_lib = create_dependency_bundle(
        "math_utils",
        "0.5.0",
        &["deps/math/vector.nova", "deps/math/matrix.nova"]
    )?;
    
    // 2. Analyze main project with dependencies
    let dependencies = vec![std_lib, math_lib];
    
    analyze_with_dependencies(
        &["src/main.nova", "src/game.nova"],
        dependencies
    )
}
```

## Dependency Resolution Patterns

### 1. File-Based Dependency Resolution

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

// Bundle manifest file (bundle.toml)
#[derive(Serialize, Deserialize)]
struct BundleManifest {
    name: String,
    version: String,
    dependencies: Vec<DependencySpec>,
    source_files: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct DependencySpec {
    name: String,
    version: String,
    path: Option<String>,    // Local path dependency
    git: Option<String>,     // Git repository dependency
    registry: Option<String>, // Package registry dependency
}

fn resolve_dependencies_from_manifest(
    manifest_path: &str
) -> Result<Vec<Bundle>, Box<dyn std::error::Error>> {
    
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: BundleManifest = toml::from_str(&manifest_content)?;
    
    let mut resolved_bundles = Vec::new();
    
    for dep_spec in manifest.dependencies {
        let bundle = match (dep_spec.path, dep_spec.git) {
            // Local path dependency
            (Some(path), None) => {
                let dep_manifest_path = PathBuf::from(path).join("bundle.toml");
                let dep_manifest: BundleManifest = toml::from_str(
                    &fs::read_to_string(dep_manifest_path)?
                )?;
                create_bundle_from_manifest(&dep_manifest, &path)?
            }
            
            // Git dependency (would need git operations)
            (None, Some(git_url)) => {
                // Clone repo, read manifest, build bundle
                todo!("Git dependency resolution")
            }
            
            // Registry dependency (would need package manager)
            _ => {
                todo!("Registry dependency resolution")
            }
        };
        
        resolved_bundles.push(bundle);
    }
    
    Ok(resolved_bundles)
}

fn create_bundle_from_manifest(
    manifest: &BundleManifest,
    base_path: &str
) -> Result<Bundle, Box<dyn std::error::Error>> {
    
    let mut compilation_units = Vec::new();
    
    for source_file in &manifest.source_files {
        let file_path = PathBuf::from(base_path).join(source_file);
        let source_code = fs::read_to_string(file_path)?;
        let tokens = Lexer::new(&source_code).lex_all()?;
        let chunk = Parser::new(tokens).parse_chunk()?;
        compilation_units.push(chunk);
    }
    
    Ok(Bundle {
        name: BundleName::from(&manifest.name),
        version: Version::parse(&manifest.version)?,
        compilation_units,
        dependencies: Vec::new(),
        exports: NamespaceExports {
            exported_definitions: HashMap::new(),
        },
        metadata: BundleMetadata::default(),
    })
}
```

### 2. Programmatic Bundle Creation

```rust
// Builder pattern for creating bundles programmatically
struct BundleBuilder {
    name: String,
    version: String,
    source_files: Vec<String>,
    dependencies: Vec<BundleDependency>,
}

impl BundleBuilder {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            source_files: Vec::new(),
            dependencies: Vec::new(),
        }
    }
    
    pub fn add_source_file(mut self, file_path: &str) -> Self {
        self.source_files.push(file_path.to_string());
        self
    }
    
    pub fn add_dependency(mut self, name: &str, version: &str) -> Self {
        self.dependencies.push(BundleDependency {
            name: BundleName::from(name),
            version_constraint: VersionConstraint::Compatible(Version::parse(version).unwrap()),
            visibility: DependencyVisibility::Private,
        });
        self
    }
    
    pub fn build(self) -> Result<Bundle, Box<dyn std::error::Error>> {
        let mut compilation_units = Vec::new();
        
        for file_path in self.source_files {
            let source_code = fs::read_to_string(&file_path)?;
            let tokens = Lexer::new(&source_code).lex_all()?;
            let chunk = Parser::new(tokens).parse_chunk()?;
            compilation_units.push(chunk);
        }
        
        Ok(Bundle {
            name: BundleName::from(&self.name),
            version: Version::parse(&self.version)?,
            compilation_units,
            dependencies: self.dependencies,
            exports: NamespaceExports {
                exported_definitions: HashMap::new(),
            },
            metadata: BundleMetadata::default(),
        })
    }
}

// Usage example
fn create_game_project() -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    // Create dependency bundles
    let std_bundle = BundleBuilder::new("std", "1.0.0")
        .add_source_file("stdlib/core.nova")
        .add_source_file("stdlib/collections.nova")
        .build()?;
    
    let graphics_bundle = BundleBuilder::new("graphics", "2.1.0")
        .add_source_file("deps/graphics/renderer.nova")
        .add_source_file("deps/graphics/sprites.nova")
        .add_dependency("std", "1.0.0")
        .build()?;
    
    // Create main project
    let main_files = &[
        "src/main.nova",
        "src/game_logic.nova",
        "src/entities.nova"
    ];
    
    analyze_with_dependencies(main_files, vec![std_bundle, graphics_bundle])
}
```

## Command-Line Interface Usage

### CLI Integration Example

```rust
use clap::{App, Arg, SubCommand};

fn main() {
    let matches = App::new("nova")
        .version("1.0.0")
        .about("Nova Programming Language Compiler")
        .subcommand(SubCommand::with_name("check")
            .about("Check source files for errors")
            .arg(Arg::with_name("files")
                .multiple(true)
                .required(true)
                .help("Nova source files to check")))
        .subcommand(SubCommand::with_name("build")
            .about("Build a Nova bundle")
            .arg(Arg::with_name("manifest")
                .short("m")
                .long("manifest")
                .value_name("FILE")
                .help("Bundle manifest file")
                .default_value("bundle.toml")))
        .get_matches();

    match matches.subcommand() {
        ("check", Some(sub_matches)) => {
            let files: Vec<&str> = sub_matches.values_of("files").unwrap().collect();
            check_files(&files);
        }
        ("build", Some(sub_matches)) => {
            let manifest = sub_matches.value_of("manifest").unwrap();
            build_bundle(manifest);
        }
        _ => {
            eprintln!("Use --help for usage information");
        }
    }
}

fn check_files(files: &[&str]) {
    match analyze_bundle_from_files(files) {
        Ok(model) => {
            println!("✅ All files passed semantic analysis");
            
            // Print comprehensive report
            print_semantic_analysis_report(&model);
        }
        Err(diagnostics) => {
            println!("❌ Found {} issues:", diagnostics.len());
            
            for diagnostic in diagnostics {
                println!("{}:{} - {}: {}",
                    diagnostic.location.line,
                    diagnostic.location.column,
                    match diagnostic.severity {
                        DiagnosticSeverity::Error => "Error",
                        DiagnosticSeverity::Warning => "Warning", 
                        DiagnosticSeverity::Info => "Info",
                    },
                    diagnostic.message
                );
            }
            
            std::process::exit(1);
        }
    }
}

fn build_bundle(manifest_path: &str) {
    match resolve_dependencies_from_manifest(manifest_path) {
        Ok(dependencies) => {
            println!("✅ Resolved {} dependencies", dependencies.len());
            
            // Continue with main bundle analysis...
            // Implementation would analyze main bundle with resolved dependencies
        }
        Err(e) => {
            eprintln!("❌ Failed to resolve dependencies: {}", e);
            std::process::exit(1);
        }
    }
}
```

## Integration Examples

### IDE/Language Server Integration

```rust
use tower_lsp::{LspService, Server};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

struct NovaLanguageServer {
    semantic_cache: HashMap<Url, SemanticModel>,
}

impl NovaLanguageServer {
    async fn analyze_document(&mut self, uri: &Url, text: &str) -> Result<()> {
        match analyze_source_file(text) {
            Ok(model) => {
                self.semantic_cache.insert(uri.clone(), model);
                
                // Send empty diagnostics (no errors)
                self.client.publish_diagnostics(uri.clone(), vec![], None).await;
            }
            Err(diagnostics) => {
                // Convert semantic diagnostics to LSP diagnostics
                let lsp_diagnostics = diagnostics.into_iter()
                    .map(|d| Diagnostic {
                        range: Range::new(
                            Position::new(d.location.line as u64, d.location.column as u64),
                            Position::new(d.location.line as u64, d.location.column as u64 + 10)
                        ),
                        severity: Some(match d.severity {
                            DiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
                            DiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
                            DiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
                        }),
                        message: d.message,
                        ..Default::default()
                    })
                    .collect();
                
                self.client.publish_diagnostics(uri.clone(), lsp_diagnostics, None).await;
            }
        }
        
        Ok(())
    }
}
```

### Build System Integration

```rust
// Integration with build systems like Cargo-style
pub struct NovaBuildConfig {
    pub bundle_name: String,
    pub version: String,
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
    pub dependencies: Vec<DependencySpec>,
    pub features: HashMap<String, bool>,
}

impl NovaBuildConfig {
    pub fn build(&self) -> Result<BuildArtifacts, BuildError> {
        // 1. Resolve dependencies
        let dependencies = self.resolve_all_dependencies()?;
        
        // 2. Collect source files
        let source_files = self.collect_source_files()?;
        
        // 3. Run semantic analysis
        let semantic_model = analyze_with_dependencies(&source_files, dependencies)?;
        
        // 4. Generate artifacts
        let artifacts = self.generate_build_artifacts(&semantic_model)?;
        
        Ok(artifacts)
    }
    
    fn resolve_all_dependencies(&self) -> Result<Vec<Bundle>, BuildError> {
        // Dependency resolution logic
        // - Check local cache
        // - Download from registries
        // - Build from source
        todo!()
    }
}
```

## Best Practices

### 1. Error Handling
```rust
// Always handle semantic analysis errors properly
match analyze_bundle(chunks) {
    Ok(model) => {
        // Success: use semantic model
        process_semantic_model(model);
    }
    Err(diagnostics) => {
        // Handle errors with appropriate detail
        for diagnostic in diagnostics {
            match diagnostic.severity {
                DiagnosticSeverity::Error => {
                    eprintln!("Error: {}", diagnostic.message);
                    // Critical errors should stop compilation
                }
                DiagnosticSeverity::Warning => {
                    eprintln!("Warning: {}", diagnostic.message); 
                    // Warnings can often be ignored in development
                }
                DiagnosticSeverity::Info => {
                    if verbose_mode {
                        println!("Info: {}", diagnostic.message);
                    }
                }
            }
        }
    }
}
```

### 2. Performance Optimization
```rust
// Cache parsed bundles for repeated use
struct BundleCache {
    parsed_bundles: HashMap<(BundleName, Version), Bundle>,
    source_hashes: HashMap<PathBuf, u64>,
}

impl BundleCache {
    fn get_or_parse_bundle(&mut self, spec: &DependencySpec) -> Result<Bundle, Error> {
        let cache_key = (spec.name.clone(), spec.version.clone());
        
        if let Some(cached) = self.parsed_bundles.get(&cache_key) {
            return Ok(cached.clone());
        }
        
        // Parse and cache
        let bundle = parse_bundle_from_spec(spec)?;
        self.parsed_bundles.insert(cache_key, bundle.clone());
        Ok(bundle)
    }
}
```

### 3. Incremental Analysis
```rust
// Track file modifications for incremental compilation
fn analyze_incrementally(
    previous_model: Option<SemanticModel>,
    changed_files: &[PathBuf],
    all_files: &[PathBuf]
) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    
    if let Some(prev_model) = previous_model {
        // Check if we can do incremental update
        if changed_files.len() == 1 && is_isolated_change(&changed_files[0]) {
            return update_semantic_model_incrementally(prev_model, &changed_files[0]);
        }
    }
    
    // Fall back to full analysis
    analyze_bundle_from_files(all_files.iter().map(|p| p.to_str().unwrap()).collect::<Vec<_>>().as_slice())
}
```

This comprehensive usage guide shows how to integrate Nova's semantic analysis system into various scenarios, from simple single-file analysis to complex multi-bundle projects with dependency resolution. The system is designed to be flexible and can be adapted to different build systems, IDEs, and development workflows.