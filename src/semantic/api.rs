use crate::semantic::{analyze_bundle, SemanticModel, SemanticDiagnostic, DiagnosticSeverity};
use crate::lexical::lexer::Lexer;
use crate::syntax::parser::Parser;
use crate::lexical::token::Position;
use std::fs;
use std::path::Path;
use std::collections::HashMap;

/// High-level API for analyzing Nova source files
pub struct NovaAnalyzer {
    /// Cache of previously analyzed bundles for performance
    bundle_cache: HashMap<String, SemanticModel>,
    /// Whether to enable verbose diagnostic output
    verbose: bool,
}

impl NovaAnalyzer {
    /// Create a new Nova analyzer
    pub fn new() -> Self {
        Self {
            bundle_cache: HashMap::new(),
            verbose: false,
        }
    }

    /// Enable verbose diagnostic output
    pub fn with_verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Analyze a single Nova source string
    pub fn analyze_source(&mut self, source: &str) -> AnalysisResult {
        match self.analyze_source_internal(source) {
            Ok(model) => AnalysisResult::Success(model),
            Err(diagnostics) => AnalysisResult::Errors(diagnostics),
        }
    }

    /// Analyze multiple Nova source files as a single bundle
    pub fn analyze_files(&mut self, file_paths: &[impl AsRef<Path>]) -> AnalysisResult {
        let mut chunks = Vec::new();
        let mut parse_errors = Vec::new();

        // Parse all files
        for file_path in file_paths {
            let path = file_path.as_ref();
            let source_code = match fs::read_to_string(path) {
                Ok(content) => content,
                Err(e) => {
                    parse_errors.push(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!("Failed to read file {:?}: {}", path, e),
                        location: Position::start(),
                        category: crate::semantic::DiagnosticCategory::SymbolResolution,
                    });
                    continue;
                }
            };

            // Lex and parse
            let tokens = match Lexer::new(&source_code).lex_all() {
                Ok(tokens) => tokens,
                Err(e) => {
                    parse_errors.push(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!("Lexer error in {:?}: {}", path, e.message),
                        location: Position::start(), // TODO: fix this
                        category: crate::semantic::DiagnosticCategory::SymbolResolution,
                    });
                    continue;
                }
            };

            let chunk = match Parser::new(tokens).parse_chunk() {
                Ok(chunk) => chunk,
                Err(e) => {
                    parse_errors.push(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!("Parser error in {:?}: {}", path, e.message),
                        location: Position::start(), // TODO: fix this
                        category: crate::semantic::DiagnosticCategory::SymbolResolution,
                    });
                    continue;
                }
            };

            chunks.push(chunk);
        }

        // If we have parse errors, return them
        if !parse_errors.is_empty() {
            return AnalysisResult::Errors(parse_errors);
        }

        // If no chunks were successfully parsed, that's an error
        if chunks.is_empty() {
            return AnalysisResult::Errors(vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "No source files could be parsed".to_string(),
                location: Position::start(),
                category: crate::semantic::DiagnosticCategory::SymbolResolution,
            }]);
        }

        // Run semantic analysis
        match analyze_bundle(chunks) {
            Ok(model) => AnalysisResult::Success(model),
            Err(diagnostics) => AnalysisResult::Errors(diagnostics),
        }
    }

    /// Analyze a directory containing Nova source files
    pub fn analyze_directory(&mut self, dir_path: impl AsRef<Path>) -> AnalysisResult {
        let dir_path = dir_path.as_ref();
        
        // Find all .nova files in the directory
        let nova_files = match self.find_nova_files(dir_path) {
            Ok(files) => files,
            Err(e) => {
                return AnalysisResult::Errors(vec![SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Failed to scan directory {:?}: {}", dir_path, e),
                    location: Position::start(),
                    category: crate::semantic::DiagnosticCategory::SymbolResolution,
                }]);
            }
        };

        if nova_files.is_empty() {
            return AnalysisResult::Errors(vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("No .nova files found in directory {:?}", dir_path),
                location: Position::start(),
                category: crate::semantic::DiagnosticCategory::SymbolResolution,
            }]);
        }

        if self.verbose {
            println!("Found {} Nova source files in {:?}", nova_files.len(), dir_path);
            for file in &nova_files {
                println!("  - {:?}", file);
            }
        }

        self.analyze_files(&nova_files)
    }

    /// Internal analysis implementation
    fn analyze_source_internal(&mut self, source: &str) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
        // Lex the source
        let tokens = Lexer::new(source).lex_all()
            .map_err(|e| vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Lex error: {}", e.message),
                location: Position::start(), // TODO: fix this
                category: crate::semantic::DiagnosticCategory::SymbolResolution,
            }])?;

        // Parse into AST
        let chunk = Parser::new(tokens).parse_chunk()
            .map_err(|e| vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Parse error: {}", e.message),
                location: Position::start(), // TODO: fix this
                category: crate::semantic::DiagnosticCategory::SymbolResolution,
            }])?;

        // Run semantic analysis
        analyze_bundle(vec![chunk])
    }

    /// Find all .nova files in a directory recursively
    fn find_nova_files(&self, dir_path: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        let mut nova_files = Vec::new();
        
        if dir_path.is_dir() {
            for entry in fs::read_dir(dir_path)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_dir() {
                    // Recursively search subdirectories
                    nova_files.extend(self.find_nova_files(&path)?);
                } else if path.extension().and_then(|s| s.to_str()) == Some("nova") {
                    nova_files.push(path);
                }
            }
        }
        
        Ok(nova_files)
    }

    /// Print a comprehensive analysis report
    pub fn print_analysis_report(&self, result: &AnalysisResult) {
        match result {
            AnalysisResult::Success(model) => {
                self.print_success_report(model);
            }
            AnalysisResult::Errors(diagnostics) => {
                self.print_error_report(diagnostics);
            }
        }
    }

    fn print_success_report(&self, model: &SemanticModel) {
        println!("🎯 Nova Semantic Analysis - SUCCESS");
        println!("┌─────────────────────────────────────────────┐");
        println!("│ Bundle: {} (v{})", model.bundle.name, model.bundle.version);
        
        // Namespace summary
        let ns_count = model.namespace_tree.namespaces.len();
        println!("│ Namespaces: {} total", ns_count);
        
        // Symbol summary  
        let export_count = model.symbol_table.exported_symbols.len();
        println!("│ Exported symbols: {}", export_count);
        
        // Type system summary
        if let Some(type_system) = &model.type_environment.type_system {
            let type_count = type_system.get_type_count();
            let primitive_count = type_system.get_primitive_type_count();
            println!("│ Types: {} user + {} primitives", type_count, primitive_count);
        }
        
        // Trait system summary
        if let Some(trait_system) = &model.trait_environment.trait_system {
            let stats = trait_system.get_statistics();
            if stats.trait_count > 0 {
                println!("│ Traits: {} definitions, {} implementations", 
                    stats.trait_count, stats.trait_implementation_count);
            }
        }
        
        // Decorator summary
        if let Some(decorator_system) = &model.decorator_environment.decorator_system {
            let stats = decorator_system.get_statistics();
            if stats.resolved_applications > 0 {
                println!("│ Decorators: {} applications, {} metadata entries", 
                    stats.resolved_applications, stats.metadata_entries);
            }
        }
        
        // Visibility summary
        if let Some(visibility_system) = &model.visibility_environment.visibility_system {
            let stats = visibility_system.get_statistics();
            println!("│ Visibility: {} public, {} private definitions", 
                stats.public_definitions, stats.private_definitions);
        }
        
        // Diagnostic summary
        if let Some(diagnostic_system) = &model.diagnostic_environment.diagnostic_system {
            let stats = diagnostic_system.get_statistics();
            if stats.total_diagnostics > 0 {
                println!("│ Diagnostics: {} processed", stats.total_diagnostics);
            }
        }
        
        println!("└─────────────────────────────────────────────┘");
        println!("✅ Semantic analysis completed successfully!");
    }

    fn print_error_report(&self, diagnostics: &[SemanticDiagnostic]) {
        println!("❌ Nova Semantic Analysis - FAILED");
        println!("Found {} issues:", diagnostics.len());
        
        let mut error_count = 0;
        let mut warning_count = 0;
        let mut info_count = 0;
        
        for diagnostic in diagnostics {
            match diagnostic.severity {
                DiagnosticSeverity::Error => {
                    error_count += 1;
                    println!("  ❌ Error: {}", diagnostic.message);
                }
                DiagnosticSeverity::Warning => {
                    warning_count += 1;
                    println!("  ⚠️  Warning: {}", diagnostic.message);
                }
                DiagnosticSeverity::Info => {
                    info_count += 1;
                    if self.verbose {
                        println!("  ℹ️  Info: {}", diagnostic.message);
                    }
                }
            }
        }
        
        println!("\nSummary: {} errors, {} warnings, {} info", 
            error_count, warning_count, info_count);
    }
}

/// Result of semantic analysis
pub enum AnalysisResult {
    /// Analysis completed successfully
    Success(SemanticModel),
    /// Analysis failed with diagnostics
    Errors(Vec<SemanticDiagnostic>),
}

impl Default for NovaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// Convenience functions for simple use cases

/// Analyze a single Nova source string (convenience function)
pub fn analyze_nova_source(source: &str) -> AnalysisResult {
    NovaAnalyzer::new().analyze_source(source)
}

/// Analyze Nova source files (convenience function)
pub fn analyze_nova_files(file_paths: &[impl AsRef<Path>]) -> AnalysisResult {
    NovaAnalyzer::new().analyze_files(file_paths)
}

/// Analyze a directory of Nova files (convenience function)
pub fn analyze_nova_directory(dir_path: impl AsRef<Path>) -> AnalysisResult {
    NovaAnalyzer::new().analyze_directory(dir_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_source_analysis() {
        let source = r#"
        namespace Test;
        export define hello(): unit
            println("Hello, Nova!")
        end
        "#;

        let result = analyze_nova_source(source);
        match result {
            AnalysisResult::Success(model) => {
                assert!(!model.symbol_table.exported_symbols.is_empty());
                assert!(model.namespace_tree.namespaces.len() >= 2);
            }
            AnalysisResult::Errors(diagnostics) => {
                panic!("Expected success but got {} errors", diagnostics.len());
            }
        }
    }

    #[test]
    fn test_error_handling() {
        let source_with_error = r#"
        namespace Test;
        define broken(): unit
            return undefined_variable
        end
        "#;

        let result = analyze_nova_source(source_with_error);
        match result {
            AnalysisResult::Success(_) => {
                panic!("Expected errors but analysis succeeded");
            }
            AnalysisResult::Errors(diagnostics) => {
                assert!(!diagnostics.is_empty());
                let has_error = diagnostics.iter()
                    .any(|d| d.severity == DiagnosticSeverity::Error);
                assert!(has_error);
            }
        }
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = NovaAnalyzer::new();
        assert!(!analyzer.verbose);
        assert!(analyzer.bundle_cache.is_empty());

        let verbose_analyzer = NovaAnalyzer::new().with_verbose();
        assert!(verbose_analyzer.verbose);
    }
}