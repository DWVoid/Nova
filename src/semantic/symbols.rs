//! Symbol table construction and cross-reference resolution
//!
//! This module implements the global symbol table system that resolves symbols across
//! namespaces and bundles, handles symbol conflicts, and provides cross-reference
//! resolution for the Nova semantic analysis system.

use crate::syntax::ast::{TopItem, Definition, DefExpr, Exp};
use crate::lexical::{Position, Span};
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName, ExportedSymbol, ImportedSymbol, ResolutionStatus, UnresolvedReason, SymbolConflict, Definition as SemanticDefinition, Visibility};
use super::bundle::BundleName;
use super::namespace::{NamespaceTree, NamespacePath, DefinitionReference, DefinitionKind};
use std::collections::{HashMap, HashSet};

/// Global symbol table managing symbols across all bundles
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SymbolTableBuilder {
    /// Current bundle being processed
    current_bundle: Option<BundleName>,
    /// Exported symbols from all analyzed bundles
    exported_symbols: HashMap<QualifiedName, ExportedSymbol>,
    /// Imported symbols and their resolution status
    imported_symbols: HashMap<(BundleName, QualifiedName), ImportedSymbol>,
    /// Detected symbol conflicts
    symbol_conflicts: Vec<SymbolConflict>,
    /// Symbol visibility cache for fast lookup
    visibility_cache: HashMap<QualifiedName, Vec<BundleName>>,
}

/// Symbol resolution context for a specific lookup
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SymbolResolutionContext {
    /// Bundle requesting the symbol
    requesting_bundle: BundleName,
    /// Namespace where the symbol is being accessed
    requesting_namespace: NamespacePath,
    /// Available imported symbols in the current scope
    available_imports: HashMap<String, DefinitionReference>,
    /// Local symbols in the current namespace
    local_symbols: HashMap<String, DefinitionReference>,
}

/// Result of symbol resolution with detailed information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum SymbolResolutionResult {
    /// Symbol successfully resolved
    Resolved {
        definition_ref: DefinitionReference,
        resolution_path: Vec<ResolutionStep>,
        access_level: AccessLevel,
    },
    /// Symbol resolution failed
    Failed {
        reason: UnresolvedReason,
        candidates: Vec<DefinitionReference>,
        suggestions: Vec<String>,
    },
    /// Multiple valid candidates found
    Ambiguous {
        candidates: Vec<DefinitionReference>,
        disambiguation_hint: Option<String>,
    },
}

/// Steps taken during symbol resolution
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum ResolutionStep {
    /// Checked local namespace
    LocalLookup(NamespacePath),
    /// Checked imported symbols
    ImportLookup(String, BundleName),
    /// Checked parent namespace
    ParentLookup(NamespacePath),
    /// Checked global exports
    GlobalLookup(BundleName),
    /// Applied visibility filtering
    VisibilityFilter(Visibility),
}

/// Access level for a resolved symbol
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum AccessLevel {
    /// Direct access to local symbol
    Local,
    /// Access through imports
    Imported,
    /// Access to public symbol from another bundle
    Public,
    /// Restricted access (warnings may apply)
    Restricted,
}

impl SymbolTableBuilder {
    /// Create a new symbol table builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Build symbol table from namespace tree and bundle information
    pub fn build_from_namespace_tree(
        bundle_name: BundleName,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) -> HashMap<QualifiedName, ExportedSymbol> {
        let mut builder = Self::new();
        builder.current_bundle = Some(bundle_name.clone());

        // Phase 1: Collect all definitions from the namespace tree
        builder.collect_definitions(namespace_tree, diagnostics);

        // Phase 2: Resolve cross-references within the bundle
        builder.resolve_internal_references(namespace_tree, diagnostics);

        // Phase 3: Build export table
        builder.build_export_table(namespace_tree, diagnostics);

        // Phase 4: Validate symbol consistency
        builder.validate_symbol_consistency(diagnostics);

        builder.exported_symbols
    }

    /// Collect all definitions from namespace tree
    fn collect_definitions(
        &mut self,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for (symbol_name, local_def) in &namespace_scope.definitions {
                let qualified_name = QualifiedName {
                    bundle: namespace_tree.root.clone(),
                    namespace: namespace_path.0.clone(),
                    name: symbol_name.clone(),
                };

                // Convert AST definition to semantic definition
                if let Some(semantic_def) = self.convert_ast_to_semantic_definition(&local_def.item) {
                    let exported_symbol = ExportedSymbol {
                        definition: semantic_def,
                        source_bundle: namespace_tree.root.clone(),
                        visibility_constraints: Vec::new(), // TODO: Extract from visibility
                        mangled_name: self.mangle_symbol_name(&qualified_name),
                    };

                    // Check for conflicts
                    if self.exported_symbols.contains_key(&qualified_name) {
                        self.report_symbol_conflict(&qualified_name, local_def.span, diagnostics);
                    } else {
                        self.exported_symbols.insert(qualified_name, exported_symbol);
                    }
                }
            }
        }
    }

    /// Convert AST definition to semantic definition
    fn convert_ast_to_semantic_definition(&self, item: &TopItem) -> Option<SemanticDefinition> {
        match item {
            TopItem::Definition(def) => {
                let visibility = self.extract_visibility(def);
                
                match &def.expr {
                    DefExpr::Struct(_) | DefExpr::Enum(_) | DefExpr::Variant(_) | DefExpr::Trait(_) => {
                        // For now, create a placeholder type definition
                        Some(SemanticDefinition::Type {
                            type_def: super::TypeDefinition::Struct {
                                fields: HashMap::new(),
                                type_parameters: Vec::new(),
                            },
                            visibility,
                        })
                    }
                    DefExpr::Exp(exp) => {
                        match exp {
                            Exp::Lambda(_) => {
                                Some(SemanticDefinition::Function {
                                    signature: super::FunctionSignature {
                                        name: def.name.value.clone(),
                                        type_parameters: Vec::new(),
                                        parameters: Vec::new(),
                                        return_type: super::Type::Primitive(super::PrimitiveType::Unit),
                                        is_const: false,
                                    },
                                    visibility,
                                })
                            }
                            _ => {
                                Some(SemanticDefinition::Value {
                                    value_type: super::Type::Primitive(super::PrimitiveType::Unit), // Placeholder
                                    visibility,
                                    is_mutable: false, // TODO: Determine from definition
                                })
                            }
                        }
                    }
                }
            }
            TopItem::Implementation(_) => {
                // Implementation blocks don't create direct symbols
                None
            }
        }
    }

    /// Extract visibility from definition
    fn extract_visibility(&self, def: &Definition) -> Visibility {
        if def.visibility.is_some() {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    /// Resolve internal cross-references within the bundle
    fn resolve_internal_references(
        &mut self,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // For each import in each namespace, try to resolve it
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for import in &namespace_scope.imports {
                for item in &import.imported_items {
                    let qualified_name = QualifiedName {
                        bundle: import.source_bundle.clone(),
                        namespace: import.source_namespace.0.clone(),
                        name: item.original_name.clone(),
                    };

                    let imported_symbol = ImportedSymbol {
                        original_symbol: qualified_name.clone(),
                        source_bundle: import.source_bundle.clone(),
                        local_name: item.local_name.clone(),
                        resolution_status: if import.source_bundle == namespace_tree.root {
                            // Internal reference - try to resolve now
                            if let Some(exported) = self.exported_symbols.get(&qualified_name) {
                                ResolutionStatus::Resolved(exported.clone())
                            } else {
                                ResolutionStatus::Unresolved(UnresolvedReason::SymbolNotFound)
                            }
                        } else {
                            // External reference - will be resolved at link time
                            ResolutionStatus::Unresolved(UnresolvedReason::SymbolNotFound)
                        },
                    };

                    let import_key = (namespace_tree.root.clone(), qualified_name);
                    self.imported_symbols.insert(import_key, imported_symbol);
                }
            }

            // Report unresolved internal imports as warnings
            for import in &namespace_scope.imports {
                if import.source_bundle == namespace_tree.root {
                    // This is an internal import
                    for item in &import.imported_items {
                        let qualified_name = QualifiedName {
                            bundle: import.source_bundle.clone(),
                            namespace: import.source_namespace.0.clone(),
                            name: item.original_name.clone(),
                        };

                        if !self.exported_symbols.contains_key(&qualified_name) {
                            diagnostics.push(SemanticDiagnostic {
                                severity: DiagnosticSeverity::Warning,
                                message: format!(
                                    "Unresolved internal import: '{}' in namespace '{}'",
                                    qualified_name.name,
                                    namespace_path
                                ),
                                location: import.span.start,
                                category: DiagnosticCategory::SymbolResolution,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Build export table for the bundle
    fn build_export_table(
        &mut self,
        namespace_tree: &NamespaceTree,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Update visibility cache
        for (qualified_name, exported_symbol) in &self.exported_symbols {
            let visibility_scopes = match &exported_symbol.definition {
                SemanticDefinition::Type { visibility, .. } |
                SemanticDefinition::Value { visibility, .. } |
                SemanticDefinition::Function { visibility, .. } => {
                    match visibility {
                        Visibility::Public => vec![], // Public to all
                        Visibility::BundlePrivate => vec![namespace_tree.root.clone()],
                        Visibility::Private => vec![namespace_tree.root.clone()],
                        Visibility::Restricted(bundles) => bundles.clone(),
                    }
                }
            };
            
            self.visibility_cache.insert(qualified_name.clone(), visibility_scopes);
        }
    }

    /// Validate symbol consistency and detect conflicts
    fn validate_symbol_consistency(&mut self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        // Check for any remaining conflicts in our symbol conflicts list
        for conflict in &self.symbol_conflicts {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "Symbol conflict for '{}': {} definitions found",
                    conflict.symbol.name,
                    conflict.conflicting_symbols.len()
                ),
                location: conflict.location,
                category: DiagnosticCategory::SymbolResolution,
            });
        }

        // Validate that all exported symbols have proper definitions
        let mut invalid_exports = Vec::new();
        for (qualified_name, exported_symbol) in &self.exported_symbols {
            if exported_symbol.mangled_name.is_empty() {
                invalid_exports.push(qualified_name.clone());
            }
        }

        for invalid_export in invalid_exports {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Invalid export: symbol '{}' has no mangled name", invalid_export.name),
                location: Position::new_start(),
                category: DiagnosticCategory::SymbolResolution,
            });
        }
    }

    /// Report a symbol conflict
    fn report_symbol_conflict(
        &mut self,
        qualified_name: &QualifiedName,
        location: Span,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        diagnostics.push(SemanticDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: format!("Duplicate symbol definition: '{}'", qualified_name.name),
            location: location.start,
            category: DiagnosticCategory::SymbolResolution,
        });
    }

    /// Generate mangled name for symbol
    fn mangle_symbol_name(&self, qualified_name: &QualifiedName) -> String {
        // Simple name mangling scheme: bundle::namespace::name
        format!(
            "{}::{}::{}",
            qualified_name.bundle.to_string(),
            qualified_name.namespace.join("::"),
            qualified_name.name
        )
    }

    /// Resolve a symbol in a specific context
    #[allow(dead_code)]
    pub fn resolve_symbol(
        &self,
        symbol_name: &str,
        context: &SymbolResolutionContext,
    ) -> SymbolResolutionResult {
        let mut resolution_steps = Vec::new();

        // Step 1: Check local symbols
        resolution_steps.push(ResolutionStep::LocalLookup(context.requesting_namespace.clone()));
        if let Some(local_ref) = context.local_symbols.get(symbol_name) {
            return SymbolResolutionResult::Resolved {
                definition_ref: local_ref.clone(),
                resolution_path: resolution_steps,
                access_level: AccessLevel::Local,
            };
        }

        // Step 2: Check available imports
        resolution_steps.push(ResolutionStep::ImportLookup(symbol_name.to_string(), context.requesting_bundle.clone()));
        if let Some(import_ref) = context.available_imports.get(symbol_name) {
            return SymbolResolutionResult::Resolved {
                definition_ref: import_ref.clone(),
                resolution_path: resolution_steps,
                access_level: AccessLevel::Imported,
            };
        }

        // Step 3: Check global exports (if public)
        resolution_steps.push(ResolutionStep::GlobalLookup(context.requesting_bundle.clone()));
        let mut candidates = Vec::new();
        
        for (qualified_name, exported_symbol) in &self.exported_symbols {
            if qualified_name.name == symbol_name {
                // Check visibility
                if self.can_access_symbol(&context.requesting_bundle, qualified_name, exported_symbol) {
                    candidates.push(DefinitionReference {
                        bundle: qualified_name.bundle.clone(),
                        namespace: NamespacePath::new(qualified_name.namespace.clone()),
                        name: qualified_name.name.clone(),
                        definition_kind: self.definition_kind_from_semantic(&exported_symbol.definition),
                    });
                }
            }
        }

        match candidates.len() {
            0 => SymbolResolutionResult::Failed {
                reason: UnresolvedReason::SymbolNotFound,
                candidates: Vec::new(),
                suggestions: self.suggest_similar_symbols(symbol_name),
            },
            1 => SymbolResolutionResult::Resolved {
                definition_ref: candidates.into_iter().next().unwrap(),
                resolution_path: resolution_steps,
                access_level: AccessLevel::Public,
            },
            _ => {
                let candidate_count = candidates.len();
                SymbolResolutionResult::Ambiguous {
                    candidates,
                    disambiguation_hint: Some(format!(
                        "Use fully qualified name to disambiguate between {} candidates",
                        candidate_count
                    )),
                }
            }
        }
    }

    /// Check if a bundle can access a specific symbol
    fn can_access_symbol(
        &self,
        requesting_bundle: &BundleName,
        qualified_name: &QualifiedName,
        exported_symbol: &ExportedSymbol,
    ) -> bool {
        match &exported_symbol.definition {
            SemanticDefinition::Type { visibility, .. } |
            SemanticDefinition::Value { visibility, .. } |
            SemanticDefinition::Function { visibility, .. } => {
                match visibility {
                    Visibility::Public => true,
                    Visibility::BundlePrivate => requesting_bundle == &qualified_name.bundle,
                    Visibility::Private => requesting_bundle == &qualified_name.bundle,
                    Visibility::Restricted(allowed_bundles) => {
                        allowed_bundles.contains(requesting_bundle)
                    }
                }
            }
        }
    }

    /// Convert semantic definition to definition kind
    fn definition_kind_from_semantic(&self, definition: &SemanticDefinition) -> DefinitionKind {
        match definition {
            SemanticDefinition::Type { .. } => DefinitionKind::Type,
            SemanticDefinition::Value { .. } => DefinitionKind::Value,
            SemanticDefinition::Function { .. } => DefinitionKind::Function,
        }
    }

    /// Suggest similar symbol names for error messages
    fn suggest_similar_symbols(&self, symbol_name: &str) -> Vec<String> {
        let mut suggestions = Vec::new();
        
        for qualified_name in self.exported_symbols.keys() {
            // Simple similarity check - same length or edit distance of 1-2
            let candidate = &qualified_name.name;
            if candidate.len() == symbol_name.len() || 
               (candidate.len() as i32 - symbol_name.len() as i32).abs() <= 2 {
                
                // Check for common patterns like case differences
                if candidate.to_lowercase() == symbol_name.to_lowercase() ||
                   candidate.starts_with(symbol_name) ||
                   symbol_name.starts_with(candidate) {
                    suggestions.push(candidate.clone());
                }
            }
        }

        // Limit suggestions to avoid overwhelming output
        suggestions.truncate(5);
        suggestions.sort();
        suggestions.dedup();
        suggestions
    }

    /// Get all exported symbols for a namespace
    #[allow(dead_code)]
    pub fn get_namespace_exports(&self, namespace_path: &NamespacePath) -> Vec<&ExportedSymbol> {
        self.exported_symbols
            .iter()
            .filter(|(qualified_name, _)| {
                qualified_name.namespace == namespace_path.0
            })
            .map(|(_, exported_symbol)| exported_symbol)
            .collect()
    }

    /// Get import dependencies for a bundle
    #[allow(dead_code)]
    pub fn get_bundle_dependencies(&self, bundle_name: &BundleName) -> HashSet<BundleName> {
        let mut dependencies = HashSet::new();
        
        for ((importing_bundle, qualified_name), _) in &self.imported_symbols {
            if importing_bundle == bundle_name && qualified_name.bundle != *bundle_name {
                dependencies.insert(qualified_name.bundle.clone());
            }
        }
        
        dependencies
    }

    /// Get all symbol names for diagnostic suggestions
    #[allow(dead_code)]
    pub fn get_all_symbol_names(&self) -> Vec<String> {
        self.exported_symbols.keys().map(|qualified| qualified.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::{Name, Exp, ExpLambda};

    fn create_test_definition(name: &str, is_exported: bool) -> TopItem {
        TopItem::Definition(Definition {
            span: Span::single(Position::new_start()),
            decorators: Vec::new(),
            visibility: if is_exported {
                Some(crate::syntax::ast::Visibility {
                    span: Span::single(Position::new_start()),
                    scopes: None,
                })
            } else {
                None
            },
            name: Name {
                value: name.to_string(),
                span: Span::single(Position::new_start()),
            },
            type_spec: None,
            expr: DefExpr::Exp(Exp::Lambda(ExpLambda {
                span: Span::single(Position::new_start()),
                lambda: crate::syntax::ast::LambdaExpr {
                    span: Span::single(Position::new_start()),
                    is_const: false,
                    params: Vec::new(),
                    return_type: crate::syntax::ast::TypeSpec {
                        span: Span::single(Position::new_start()),
                        ty: crate::syntax::ast::TypeName {
                            span: Span::single(Position::new_start()),
                            parts: vec![Name {
                                value: "unit".to_string(),
                                span: Span::single(Position::new_start()),
                            }],
                        },
                    },
                    block: crate::syntax::ast::Block {
                        span: Span::single(Position::new_start()),
                        stats: Vec::new(),
                    },
                },
            })),
        })
    }

    #[test]
    fn test_symbol_table_builder_creation() {
        let builder = SymbolTableBuilder::new();
        assert!(builder.exported_symbols.is_empty());
        assert!(builder.imported_symbols.is_empty());
        assert!(builder.symbol_conflicts.is_empty());
    }

    #[test]
    fn test_definition_conversion() {
        let builder = SymbolTableBuilder::new();
        let test_def = create_test_definition("test_func", true);
        
        let semantic_def = builder.convert_ast_to_semantic_definition(&test_def);
        assert!(semantic_def.is_some());
        
        if let Some(SemanticDefinition::Function { signature, visibility }) = semantic_def {
            assert_eq!(signature.name, "test_func");
            assert_eq!(visibility, Visibility::Public);
        } else {
            panic!("Expected function definition");
        }
    }

    #[test]
    fn test_symbol_name_mangling() {
        let builder = SymbolTableBuilder::new();
        let qualified_name = QualifiedName {
            bundle: BundleName::from("test_bundle"),
            namespace: vec!["System".to_string(), "IO".to_string()],
            name: "println".to_string(),
        };
        
        let mangled = builder.mangle_symbol_name(&qualified_name);
        assert_eq!(mangled, "test_bundle::System::IO::println");
    }

    #[test]
    fn test_symbol_similarity_suggestions() {
        let mut builder = SymbolTableBuilder::new();
        
        // Add some test symbols
        let test_symbols = vec![
            ("print", "test_bundle", vec!["System".to_string()]),
            ("println", "test_bundle", vec!["System".to_string()]),
            ("Print", "test_bundle", vec!["System".to_string()]),
        ];
        
        for (name, bundle, namespace) in test_symbols {
            let qualified_name = QualifiedName {
                bundle: BundleName::from(bundle),
                namespace,
                name: name.to_string(),
            };
            
            builder.exported_symbols.insert(qualified_name, ExportedSymbol {
                definition: SemanticDefinition::Function {
                    signature: super::super::FunctionSignature {
                        name: name.to_string(),
                        type_parameters: Vec::new(),
                        parameters: Vec::new(),
                        return_type: super::super::Type::Primitive(super::super::PrimitiveType::Unit),
                        is_const: false,
                    },
                    visibility: Visibility::Public,
                },
                source_bundle: BundleName::from(bundle),
                visibility_constraints: Vec::new(),
                mangled_name: format!("{}::{}", bundle, name),
            });
        }
        
        let suggestions = builder.suggest_similar_symbols("print");
        assert!(!suggestions.is_empty());
        assert!(suggestions.contains(&"print".to_string()));
        assert!(suggestions.contains(&"Print".to_string()) || suggestions.contains(&"println".to_string()));
    }
}