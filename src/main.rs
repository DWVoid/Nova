mod lexical;
mod syntax;
mod semantic;
mod bundle_manifest;
use crate::lexical::lexer::Lexer;
use crate::syntax::emit::Emitter;
use crate::syntax::parser::Parser;
use std::io::{self, Read};
fn main() {
    let mut input = String::new();
    if let Err(err) = io::stdin().read_to_string(&mut input) {
        eprintln!("Failed to read stdin: {err}");
        std::process::exit(1);
    }

    let tokens = match Lexer::new(&input).lex_all() {
        Ok(tokens) => tokens,
        Err(err) => {
            eprintln!("Lex error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    let chunk = match Parser::new(tokens).parse_chunk() {
        Ok(chunk) => chunk,
        Err(err) => {
            eprintln!("Parse error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    // Run semantic analysis
    match crate::semantic::analyze_bundle(vec![chunk.clone()]) {
        Ok(semantic_model) => {
            eprintln!("Semantic analysis completed for bundle: {}", semantic_model.bundle.name);
            eprintln!("Bundle version: {}", semantic_model.bundle.version);
            eprintln!("Namespace tree contains {} namespaces:", semantic_model.namespace_tree.namespaces.len());
            for (path, scope) in &semantic_model.namespace_tree.namespaces {
                eprintln!("  Namespace '{}': {} definitions, {} imports", 
                    path, 
                    scope.definitions.len(),
                    scope.imports.len()
                );
            }
            eprintln!("Symbol table contains {} exported symbols", semantic_model.symbol_table.exported_symbols.len());
            for (qualified_name, _) in &semantic_model.symbol_table.exported_symbols {
                eprintln!("  Exported: {}::{}", qualified_name.bundle, qualified_name.name);
            }
            
            if let Some(type_system) = &semantic_model.type_environment.type_system {
                let type_count = type_system.get_type_count();
                eprintln!("Type system contains {} type definitions", type_count);
                
                let type_env = type_system.get_type_environment();
                for (qualified_name, _) in &type_env.bundle_types {
                    eprintln!("  Type: {}::{}", qualified_name.bundle, qualified_name.name);
                }
                
                let primitive_count = type_system.get_primitive_type_count();
                eprintln!("Primitive types: {}", primitive_count);
            }
        }
        Err(diagnostics) => {
            eprintln!("Semantic analysis failed with {} error(s):", diagnostics.len());
            for diagnostic in diagnostics {
                eprintln!("  {}: {}", 
                    match diagnostic.severity {
                        crate::semantic::DiagnosticSeverity::Error => "Error",
                        crate::semantic::DiagnosticSeverity::Warning => "Warning",
                        crate::semantic::DiagnosticSeverity::Info => "Info",
                    },
                    diagnostic.message
                );
            }
            std::process::exit(1);
        }
    }

    let output = Emitter::emit_chunk(&chunk);
    // print!("{output}");
}
#[test]
fn test_semantic_integration() {
    let code = r#"
use System;
namespace Example;
export define add (x: integer, y: integer): integer
  return x + y
end
    "#;
    let tokens = Lexer::new(code).lex_all().unwrap();
    let chunk = Parser::new(tokens).parse_chunk().unwrap();
    match crate::semantic::analyze_bundle(vec![chunk]) {
        Ok(semantic_model) => {
            assert_eq!(semantic_model.bundle.name.to_string(), "default");
            assert_eq!(semantic_model.bundle.version.to_string(), "0.1.0");
            
            // Check namespace tree
            assert!(semantic_model.namespace_tree.namespaces.len() >= 2); // root + Example
            
            let example_path = crate::semantic::namespace::NamespacePath::new(vec!["Example".to_string()]);
            let example_scope = semantic_model.namespace_tree.namespaces.get(&example_path).unwrap();
            
            // Should have one definition (add function) and one import (System)
            assert_eq!(example_scope.definitions.len(), 1);
            assert_eq!(example_scope.imports.len(), 1);
            
            // Check the definition is exported
            assert!(example_scope.definitions.get("add").unwrap().is_exported);
        }
        Err(diagnostics) => {
            panic!("Semantic analysis failed with {} error(s)", diagnostics.len());
        }
    }
}
