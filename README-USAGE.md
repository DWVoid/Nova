# Nova Semantic Analysis System - Usage Guide

Welcome to Nova's comprehensive semantic analysis system! This guide explains exactly how to use the system to analyze Nova source code, handle dependencies, and integrate with build systems.

## Quick Start

### Basic Usage

The simplest way to use Nova's semantic analysis is through the high-level API:

```rust
use nova::semantic::api::{analyze_nova_source, AnalysisResult};

fn main() {
    let source = r#"
        namespace Example;
        @doc("A simple greeting function")
        export define greet(name: string): unit
            println("Hello, " + name + "!")
        end
    "#;

    match analyze_nova_source(source) {
        AnalysisResult::Success(model) => {
            println!("✅ Analysis successful!");
            println!("Bundle: {}", model.bundle.name);
            println!("Exports: {}", model.symbol_table.exported_symbols.len());
        }
        AnalysisResult::Errors(diagnostics) => {
            println!("❌ Analysis failed:");
            for diagnostic in diagnostics {
                println!("  {}: {}", diagnostic.severity, diagnostic.message);
            }
        }
    }
}
```

### Analyzing Multiple Files

For projects with multiple source files:

```rust
use nova::semantic::api::{analyze_nova_files, NovaAnalyzer};

fn main() {
    let files = &["src/main.nova", "src/utils.nova", "src/data.nova"];
    
    let mut analyzer = NovaAnalyzer::new().with_verbose();
    let result = analyzer.analyze_files(files);
    
    analyzer.print_analysis_report(&result);
}
```

### Command-Line Interface

We provide a complete CLI tool for analyzing Nova projects:

```bash
# Analyze specific files
nova-analyzer check src/main.nova src/utils.nova

# Analyze all files in a directory
nova-analyzer check-dir src/

# Analyze with verbose output
nova-analyzer -v check src/main.nova

# Analyze from stdin
echo 'namespace Test; export define hello(): unit; end' | nova-analyzer check-source
```

## Project Structure and Dependencies

### Bundle Configuration

Nova uses `bundle.toml` files to define projects and their dependencies:

```toml
[package]
name = "my-project"
version = "1.0.0"
description = "My Nova project"
authors = ["Your Name"]

# Source files to include
source_files = [
    "src/main.nova",
    "src/utils.nova",
    "src/models.nova"
]

# Dependencies
[[dependencies]]
name = "std"
version = "1.0.0"
path = "../stdlib"  # Local path dependency

[[dependencies]]
name = "graphics"
version = "2.1.0"
optional = true
```

### Dependency Resolution

The system supports several types of dependencies:

1. **Local Path Dependencies**
   ```toml
   [[dependencies]]
   name = "my-lib"
   version = "1.0.0"
   path = "../my-lib"
   ```

2. **Git Dependencies** (planned)
   ```toml
   [[dependencies]]
   name = "external-lib"
   version = "1.0.0"
   git = "https://github.com/example/external-lib.git"
   ```

3. **Registry Dependencies** (planned)
   ```toml
   [[dependencies]]
   name = "popular-lib"
   version = "^2.1.0"
   ```

### Bundle Manager Usage

For complex projects with dependencies:

```rust
use nova::bundle_manager::BundleManager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut manager = BundleManager::new(".")
        .with_verbose();
    
    // Analyze project with automatic dependency resolution
    let model = manager.analyze_bundle_project("bundle.toml")?;
    
    println!("🎉 Analysis complete!");
    println!("Bundle: {}", model.bundle.name);
    println!("Dependencies resolved: {}", model.bundle.dependencies.len());
    
    Ok(())
}
```

## API Reference

### High-Level API

The `nova::semantic::api` module provides the easiest way to use the system:

```rust
// Convenience functions
pub fn analyze_nova_source(source: &str) -> AnalysisResult
pub fn analyze_nova_files(files: &[impl AsRef<Path>]) -> AnalysisResult  
pub fn analyze_nova_directory(dir: impl AsRef<Path>) -> AnalysisResult

// Advanced analyzer with caching and configuration
pub struct NovaAnalyzer {
    pub fn new() -> Self
    pub fn with_verbose(self) -> Self
    pub fn analyze_source(&mut self, source: &str) -> AnalysisResult
    pub fn analyze_files(&mut self, files: &[impl AsRef<Path>]) -> AnalysisResult
    pub fn analyze_directory(&mut self, dir: impl AsRef<Path>) -> AnalysisResult
    pub fn print_analysis_report(&self, result: &AnalysisResult)
}
```

### Core Semantic Analysis API

For lower-level control, use the core API:

```rust
use nova::semantic::{analyze_bundle, analyze_bundle_with_linking};
use nova::lexical::lexer::Lexer;
use nova::syntax::parser::Parser;

// Basic single-bundle analysis
fn analyze_manually(source: &str) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    let tokens = Lexer::new(source).lex_all()?;
    let chunk = Parser::new(tokens).parse_chunk()?;
    analyze_bundle(vec![chunk])
}

// Multi-bundle analysis with dependencies
fn analyze_with_deps(
    chunks: Vec<Chunk>, 
    dependencies: Vec<Bundle>
) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    analyze_bundle_with_linking(chunks, dependencies)
}
```

## Integration Examples

### Build System Integration

```rust
// Example build script integration
use nova::semantic::api::NovaAnalyzer;
use std::process;

fn main() {
    let mut analyzer = NovaAnalyzer::new();
    
    // Check if build should proceed
    match analyzer.analyze_directory("src") {
        AnalysisResult::Success(_) => {
            println!("✅ Semantic analysis passed - proceeding with build");
            // Continue with compilation...
        }
        AnalysisResult::Errors(diagnostics) => {
            eprintln!("❌ Semantic analysis failed:");
            analyzer.print_analysis_report(&AnalysisResult::Errors(diagnostics));
            process::exit(1);
        }
    }
}
```

### IDE/Language Server Integration

```rust
// Example LSP server integration
use tower_lsp::lsp_types::*;
use nova::semantic::api::NovaAnalyzer;

struct NovaLanguageServer {
    analyzer: NovaAnalyzer,
}

impl NovaLanguageServer {
    async fn analyze_document(&mut self, uri: Url, text: String) {
        match self.analyzer.analyze_source(&text) {
            AnalysisResult::Success(_) => {
                // Send empty diagnostics (no errors)
                self.publish_diagnostics(uri, vec![]).await;
            }
            AnalysisResult::Errors(diagnostics) => {
                let lsp_diagnostics = convert_to_lsp_diagnostics(diagnostics);
                self.publish_diagnostics(uri, lsp_diagnostics).await;
            }
        }
    }
}
```

### Testing Framework Integration

```rust
// Example test runner integration
use nova::semantic::api::NovaAnalyzer;

fn run_semantic_tests() {
    let mut analyzer = NovaAnalyzer::new();
    
    // Analyze test files
    match analyzer.analyze_directory("tests/") {
        AnalysisResult::Success(model) => {
            // Extract test functions from semantic model
            let test_functions = extract_test_functions(&model);
            println!("Found {} test functions", test_functions.len());
            
            // Run tests...
        }
        AnalysisResult::Errors(diagnostics) => {
            eprintln!("❌ Test analysis failed: {} errors", diagnostics.len());
        }
    }
}

fn extract_test_functions(model: &SemanticModel) -> Vec<String> {
    // Look for functions with @test decorator
    if let Some(decorator_system) = &model.decorator_environment.decorator_system {
        // Extract test metadata...
    }
    vec![]
}
```

## Sample Project

We provide a complete sample project demonstrating all features:

```
examples/sample-project/
├── bundle.toml                 # Project configuration
├── src/
│   ├── main.nova              # Main application entry point
│   ├── math_utils.nova        # Mathematical utilities with tests
│   └── data_structures.nova   # Data structures with traits
└── README.md
```

### Running the Sample

```bash
# Analyze the sample project
cd examples/sample-project
../../target/debug/Nova -v check-dir src/

# Or use the bundle manager
cargo run --example bundle-manager bundle.toml
```

## Advanced Features

### Semantic Model Introspection

After analysis, you can inspect the complete semantic model:

```rust
match analyze_nova_files(&files) {
    AnalysisResult::Success(model) => {
        // Inspect namespaces
        for (path, scope) in &model.namespace_tree.namespaces {
            println!("Namespace {}: {} definitions", path, scope.definitions.len());
        }
        
        // Inspect exported symbols
        for (qualified_name, symbol) in &model.symbol_table.exported_symbols {
            println!("Export: {}::{}", qualified_name.bundle, qualified_name.name);
        }
        
        // Inspect type system
        if let Some(type_system) = &model.type_environment.type_system {
            println!("Types: {} user-defined", type_system.get_type_count());
        }
        
        // Inspect trait system
        if let Some(trait_system) = &model.trait_environment.trait_system {
            let stats = trait_system.get_statistics();
            println!("Traits: {} defined, {} implementations", 
                stats.trait_count, stats.trait_implementation_count);
        }
        
        // Inspect decorators
        if let Some(decorator_system) = &model.decorator_environment.decorator_system {
            let stats = decorator_system.get_statistics();
            println!("Decorators: {} applications, {} metadata entries",
                stats.resolved_applications, stats.metadata_entries);
        }
    }
    AnalysisResult::Errors(diagnostics) => {
        // Handle errors...
    }
}
```

### Performance Optimization

For large projects or frequent analysis:

```rust
// Use analyzer with caching
let mut analyzer = NovaAnalyzer::new();

// Analyze multiple times - second analysis uses cached results
let result1 = analyzer.analyze_files(&files);
let result2 = analyzer.analyze_files(&files);  // Faster due to caching

// Bundle manager with dependency caching
let mut manager = BundleManager::new(".")
    .with_verbose();

// Dependencies are cached between analyses
let model1 = manager.analyze_bundle_project("project1/bundle.toml")?;
let model2 = manager.analyze_bundle_project("project2/bundle.toml")?;  // Reuses cached deps
```

### Error Handling Best Practices

```rust
match analyze_nova_files(&files) {
    AnalysisResult::Success(model) => {
        // Process successful analysis
        process_semantic_model(model);
    }
    AnalysisResult::Errors(diagnostics) => {
        let mut error_count = 0;
        let mut warning_count = 0;
        
        for diagnostic in &diagnostics {
            match diagnostic.severity {
                DiagnosticSeverity::Error => {
                    error_count += 1;
                    eprintln!("Error: {}", diagnostic.message);
                }
                DiagnosticSeverity::Warning => {
                    warning_count += 1;
                    eprintln!("Warning: {}", diagnostic.message);
                }
                DiagnosticSeverity::Info => {
                    println!("Info: {}", diagnostic.message);
                }
            }
        }
        
        // Fail on errors, continue on warnings
        if error_count > 0 {
            std::process::exit(1);
        }
    }
}
```

## What Gets Analyzed

Nova's semantic analysis system provides comprehensive analysis across 9 phases:

1. **Bundle & Namespace Management**: Project structure and imports
2. **Symbol Resolution**: Function, type, and variable definitions
3. **Type System**: Static type checking and inference
4. **Trait System**: Interface definitions and implementations
5. **Visibility Control**: Access control and permissions
6. **Cross-Bundle Linking**: Dependency resolution (foundation)
7. **Decorator System**: Metaprogramming with @decorators
8. **Diagnostic System**: Error reporting and suggestions
9. **Integration Testing**: End-to-end validation

### What You Get

After analysis, the `SemanticModel` contains:

- **Complete type information** for all definitions
- **Resolved symbol references** across all namespaces
- **Trait implementation mappings** with method resolution
- **Decorator metadata** for code generation and tooling
- **Comprehensive diagnostics** with intelligent suggestions
- **Dependency resolution** with version compatibility
- **Performance statistics** for optimization

This makes Nova's semantic analysis one of the most comprehensive systems available, suitable for advanced IDE features, sophisticated build systems, and complex static analysis tools.

## Troubleshooting

### Common Issues

1. **File Not Found Errors**
   - Check file paths are correct and files exist
   - Use absolute paths or ensure working directory is correct

2. **Parse Errors** 
   - Check Nova syntax is valid
   - Look for missing `end` statements or syntax errors

3. **Symbol Resolution Errors**
   - Ensure all used symbols are defined or imported
   - Check namespace declarations match directory structure

4. **Type Errors**
   - Verify type annotations are correct
   - Check function return types match actual returns

5. **Dependency Resolution Errors**
   - Verify `bundle.toml` paths are correct
   - Ensure dependent bundles have valid `bundle.toml` files

### Getting Help

- Check the `examples/` directory for working examples
- Use verbose mode (`-v`) for detailed analysis output
- Review diagnostic messages for specific guidance
- The semantic model provides complete introspection capabilities

This comprehensive semantic analysis system provides everything needed for production Nova development, from simple script analysis to complex multi-bundle projects with sophisticated dependency management.