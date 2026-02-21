//! Nova Visibility and Access Control System
//!
//! This module implements Nova's visibility and access control mechanisms including
//! visibility rule enforcement, access context validation, privacy level checking,
//! and export scope restrictions. It integrates with the type system, trait system,
//! and symbol resolution to provide comprehensive access control.

use crate::syntax::ast::{TopItem, Definition, DefExpr};
use crate::lexical::token::Span;
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName};
use super::bundle::BundleName;
use super::namespace::{NamespaceTree, NamespacePath};
use super::symbols::SymbolTableBuilder;
use super::types::TypeSystem;
use super::traits::TraitSystem;
use std::collections::{HashMap, HashSet};

/// Core visibility system manager
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VisibilitySystem {
    /// Current bundle being analyzed
    bundle_name: BundleName,
    /// Visibility rules and access permissions
    visibility_table: VisibilityTable,
    /// Access context tracking
    access_contexts: Vec<AccessContext>,
    /// Visibility violation cache for performance
    violation_cache: HashMap<AccessRequest, AccessResult>,
}

/// Comprehensive visibility and access control tracking
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct VisibilityTable {
    /// Visibility rules for all definitions
    pub definition_visibility: HashMap<QualifiedName, VisibilityRule>,
    /// Bundle-level access permissions
    pub bundle_permissions: HashMap<BundleName, BundleAccessPermissions>,
    /// Namespace export scopes
    pub namespace_exports: HashMap<NamespacePath, ExportScope>,
    /// Field-level visibility for structured types
    pub field_visibility: HashMap<(QualifiedName, String), VisibilityRule>,
    /// Method visibility for implementations
    pub method_visibility: HashMap<(QualifiedName, String), VisibilityRule>,
}

/// Visibility rule specifications
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum VisibilityRule {
    /// Only accessible within the same namespace
    Private,
    /// Accessible within the same bundle
    BundlePrivate,
    /// Publicly accessible to all bundles
    Public,
    /// Accessible only to specific bundles
    Restricted(Vec<BundleName>),
    /// Friend visibility for specific relationships
    Friend(Vec<QualifiedName>),
}

/// Bundle-level access permissions
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct BundleAccessPermissions {
    /// Bundles this bundle can access
    pub accessible_bundles: HashSet<BundleName>,
    /// Bundles that can access this bundle
    pub authorized_accessors: HashSet<BundleName>,
    /// Specific permission overrides
    pub permission_overrides: HashMap<QualifiedName, AccessPermission>,
}

/// Specific access permission for a definition
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum AccessPermission {
    /// Full access granted
    Granted,
    /// Access denied
    Denied(AccessDenialReason),
    /// Conditional access based on context
    Conditional(Vec<AccessCondition>),
}

/// Reason for access denial
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum AccessDenialReason {
    /// Private visibility restriction
    PrivateAccess,
    /// Bundle-level restriction
    BundleRestriction,
    /// Namespace scope restriction
    ScopeRestriction,
    /// Deprecated definition with forced restriction
    DeprecationRestriction,
    /// Security policy violation
    SecurityPolicyViolation,
}

/// Conditions for conditional access
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum AccessCondition {
    /// Must be accessing from specific bundle
    FromBundle(BundleName),
    /// Must be accessing from specific namespace
    FromNamespace(NamespacePath),
    /// Must have specific decorator present
    WithDecorator(String),
    /// Must satisfy trait bound
    WithTraitBound(QualifiedName),
}

/// Export scope for namespaces
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ExportScope {
    /// Definitions exported from this namespace
    pub exported_definitions: HashSet<QualifiedName>,
    /// Visibility level for the entire namespace
    pub namespace_visibility: VisibilityRule,
    /// Re-export permissions for imported symbols
    pub reexport_permissions: HashMap<QualifiedName, ReexportRule>,
}

/// Re-export rule for imported symbols
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ReexportRule {
    /// Can re-export with same visibility
    SameVisibility,
    /// Can re-export with reduced visibility
    ReducedVisibility(VisibilityRule),
    /// Cannot re-export
    NoReexport,
}

/// Access context during semantic analysis
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AccessContext {
    /// Bundle making the access request
    pub accessing_bundle: BundleName,
    /// Namespace making the access request
    pub accessing_namespace: NamespacePath,
    /// Type of access being requested
    pub access_kind: AccessKind,
    /// Current definition context (if any)
    pub current_definition: Option<QualifiedName>,
    /// Stack of visibility scopes
    pub visibility_stack: Vec<VisibilityScope>,
}

/// Types of access requests
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum AccessKind {
    /// Accessing a type definition
    TypeAccess,
    /// Accessing a value or function
    ValueAccess,
    /// Calling a method
    MethodCall,
    /// Accessing a field
    FieldAccess,
    /// Implementing a trait
    TraitImplementation,
    /// Using in type position
    TypeUsage,
    /// Re-exporting a symbol
    Reexport,
}

/// Visibility scope in the access context
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VisibilityScope {
    /// Scope identifier
    pub scope_id: String,
    /// Visibility rule for this scope
    pub visibility: VisibilityRule,
    /// Whether this is a definition scope
    pub is_definition_scope: bool,
}

/// Access request for caching and analysis
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct AccessRequest {
    /// What is being accessed
    pub target: QualifiedName,
    /// Who is accessing it
    pub accessor: QualifiedName,
    /// Type of access
    pub access_kind: AccessKind,
}

/// Result of access checking
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AccessResult {
    /// Whether access is granted
    pub permission: AccessPermission,
    /// Detailed analysis of the decision
    pub analysis: AccessAnalysis,
    /// Suggestions for resolving access issues
    pub suggestions: Vec<AccessSuggestion>,
}

/// Detailed analysis of access decision
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AccessAnalysis {
    /// Visibility chain traversed
    pub visibility_chain: Vec<VisibilityStep>,
    /// Rules that applied
    pub applied_rules: Vec<String>,
    /// Context that influenced the decision
    pub decision_context: Vec<String>,
}

/// Step in visibility resolution
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum VisibilityStep {
    /// Checked definition visibility
    DefinitionCheck {
        definition: QualifiedName,
        visibility: VisibilityRule,
        result: bool,
    },
    /// Checked bundle permissions
    BundleCheck {
        source_bundle: BundleName,
        target_bundle: BundleName,
        result: bool,
    },
    /// Checked namespace scope
    NamespaceCheck {
        source_namespace: NamespacePath,
        target_namespace: NamespacePath,
        result: bool,
    },
}

/// Suggestions for resolving access violations
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum AccessSuggestion {
    /// Make definition public
    MakePublic(QualifiedName),
    /// Add bundle to restricted list
    AddBundlePermission(BundleName),
    /// Use alternative accessible symbol
    UseAlternative(QualifiedName),
    /// Add import statement
    AddImport(String),
    /// Change visibility modifier
    ChangeVisibility(QualifiedName, VisibilityRule),
}

/// Visibility violation details
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VisibilityViolation {
    /// The access that was denied
    pub denied_access: AccessRequest,
    /// Reason for denial
    pub denial_reason: AccessDenialReason,
    /// Location of the violation
    pub location: Span,
    /// Suggested fixes
    pub suggestions: Vec<AccessSuggestion>,
}

impl VisibilitySystem {
    /// Create a new visibility system for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        Self {
            bundle_name,
            visibility_table: VisibilityTable::default(),
            access_contexts: Vec::new(),
            violation_cache: HashMap::new(),
        }
    }

    /// Build visibility system from namespace tree and other semantic components
    pub fn build_from_semantic_components(
        &mut self,
        namespace_tree: &NamespaceTree,
        symbol_table: &SymbolTableBuilder,
        type_system: &TypeSystem,
        trait_system: &TraitSystem,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Phase 1: Extract visibility rules from definitions
        self.extract_visibility_rules(namespace_tree, diagnostics);

        // Phase 2: Build bundle permission matrix
        self.build_bundle_permissions(namespace_tree, symbol_table, diagnostics);

        // Phase 3: Analyze namespace export scopes
        self.analyze_namespace_exports(namespace_tree, diagnostics);

        // Phase 4: Validate field and method visibility
        self.validate_member_visibility(namespace_tree, type_system, trait_system, diagnostics);

        // Phase 5: Check for visibility violations
        self.check_visibility_violations(namespace_tree, symbol_table, diagnostics);
    }

    /// Extract visibility rules from AST definitions
    fn extract_visibility_rules(
        &mut self,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for (symbol_name, local_def) in &namespace_scope.definitions {
                if let TopItem::Definition(def) = &local_def.item {
                    let qualified_name = QualifiedName {
                        bundle: namespace_tree.root.clone(),
                        namespace: namespace_path.0.clone(),
                        name: symbol_name.clone(),
                    };

                    // Extract visibility rule from definition
                    let visibility_rule = self.extract_visibility_rule(def, namespace_path);
                    
                    // Validate visibility rule
                    if let Err(diagnostic) = self.validate_visibility_rule(&visibility_rule, &qualified_name, def) {
                        diagnostics.push(diagnostic);
                        // Use safe default
                        self.visibility_table.definition_visibility.insert(
                            qualified_name.clone(),
                            VisibilityRule::Private
                        );
                    } else {
                        self.visibility_table.definition_visibility.insert(
                            qualified_name.clone(),
                            visibility_rule
                        );
                    }

                    // Extract field visibility for structured types
                    if let DefExpr::Struct(struct_def) = &def.expr {
                        self.extract_field_visibility(&qualified_name, struct_def, diagnostics);
                    }
                }
            }

            // Process implementations for method visibility
            for implementation in &namespace_scope.implementations {
                self.extract_method_visibility(
                    &implementation.implementation,
                    namespace_path,
                    diagnostics
                );
            }
        }
    }

    /// Extract visibility rule from a definition
    fn extract_visibility_rule(&self, def: &Definition, _namespace_path: &NamespacePath) -> VisibilityRule {
        match &def.visibility {
            Some(visibility) => {
                // Check if there are specific scopes defined
                match &visibility.scopes {
                    Some(scopes) => {
                        // If specific scopes are defined, treat as restricted visibility
                        let bundle_names = scopes.iter()
                            .map(|scope| BundleName::from(scope.value.as_str()))
                            .collect();
                        VisibilityRule::Restricted(bundle_names)
                    }
                    None => {
                        // No specific scopes means public visibility
                        VisibilityRule::Public
                    }
                }
            }
            None => {
                // No explicit visibility modifier - use private as default
                VisibilityRule::Private
            }
        }
    }

    /// Validate a visibility rule for correctness
    fn validate_visibility_rule(
        &self,
        rule: &VisibilityRule,
        qualified_name: &QualifiedName,
        def: &Definition,
    ) -> Result<(), SemanticDiagnostic> {
        match rule {
            VisibilityRule::Restricted(bundles) => {
                // Check that restricted bundles are valid
                if bundles.is_empty() {
                    return Err(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "Restricted visibility for '{}' must specify at least one bundle",
                            qualified_name.name
                        ),
                        location: def.span.start,
                        category: DiagnosticCategory::VisibilityViolation,
                    });
                }
            }
            VisibilityRule::Friend(friends) => {
                // Check that friend relationships are valid
                if friends.is_empty() {
                    return Err(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!(
                            "Friend visibility for '{}' specifies no friends - consider using private instead",
                            qualified_name.name
                        ),
                        location: def.span.start,
                        category: DiagnosticCategory::VisibilityViolation,
                    });
                }
            }
            _ => {
                // Other visibility rules are always valid
            }
        }

        Ok(())
    }

    /// Extract field visibility from struct definitions
    fn extract_field_visibility(
        &mut self,
        struct_name: &QualifiedName,
        struct_def: &crate::syntax::ast::StructDef,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for field in &struct_def.fields {
            let field_key = (struct_name.clone(), field.name.value.clone());
            
            // For now, all fields default to the same visibility as their containing struct
            // This would be enhanced when field-level visibility modifiers are added
            let field_visibility = self.visibility_table.definition_visibility
                .get(struct_name)
                .cloned()
                .unwrap_or(VisibilityRule::Private);

            self.visibility_table.field_visibility.insert(field_key, field_visibility);
        }
    }

    /// Extract method visibility from implementation blocks
    fn extract_method_visibility(
        &mut self,
        impl_block: &crate::syntax::ast::Implementation,
        namespace_path: &NamespacePath,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        let impl_type = self.convert_type_name_to_qualified(&impl_block.target, namespace_path);

        for item in &impl_block.items {
            if let Definition { expr: DefExpr::Exp(_), .. } = item {
                let method_key = (impl_type.clone(), item.name.value.clone());
                
                // Extract method visibility (defaults to public for now)
                let method_visibility = match &item.visibility {
                    Some(_) => VisibilityRule::Public,
                    None => VisibilityRule::Public, // Methods default to public
                };

                self.visibility_table.method_visibility.insert(method_key, method_visibility);
            }
        }
    }

    /// Convert AST type name to qualified name
    fn convert_type_name_to_qualified(
        &self,
        type_name: &crate::syntax::ast::TypeName,
        namespace_path: &NamespacePath,
    ) -> QualifiedName {
        QualifiedName {
            bundle: self.bundle_name.clone(),
            namespace: if type_name.parts.len() > 1 {
                type_name.parts.iter().take(type_name.parts.len() - 1)
                    .map(|part| part.value.clone()).collect()
            } else {
                namespace_path.0.clone()
            },
            name: type_name.parts.last().unwrap().value.clone(),
        }
    }

    /// Build bundle permission matrix
    fn build_bundle_permissions(
        &mut self,
        namespace_tree: &NamespaceTree,
        _symbol_table: &SymbolTableBuilder,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        let bundle_permissions = BundleAccessPermissions {
            accessible_bundles: HashSet::new(), // Will be populated from dependency analysis
            authorized_accessors: HashSet::new(), // Will be populated from reverse dependencies
            permission_overrides: HashMap::new(),
        };

        self.visibility_table.bundle_permissions.insert(
            namespace_tree.root.clone(),
            bundle_permissions
        );
    }

    /// Analyze namespace export scopes
    fn analyze_namespace_exports(
        &mut self,
        namespace_tree: &NamespaceTree,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            let mut exported_definitions = HashSet::new();
            
            // Collect exported definitions from this namespace
            for (symbol_name, local_def) in &namespace_scope.definitions {
                if local_def.is_exported {
                    let qualified_name = QualifiedName {
                        bundle: namespace_tree.root.clone(),
                        namespace: namespace_path.0.clone(),
                        name: symbol_name.clone(),
                    };
                    exported_definitions.insert(qualified_name);
                }
            }

            // Determine namespace-level visibility
            let namespace_visibility = if namespace_path.0.is_empty() {
                VisibilityRule::Public // Root namespace is public
            } else {
                VisibilityRule::BundlePrivate // Other namespaces default to bundle-private
            };

            let export_scope = ExportScope {
                exported_definitions,
                namespace_visibility,
                reexport_permissions: HashMap::new(), // TODO: Analyze re-export permissions
            };

            self.visibility_table.namespace_exports.insert(namespace_path.clone(), export_scope);
        }
    }

    /// Validate field and method visibility consistency
    fn validate_member_visibility(
        &mut self,
        _namespace_tree: &NamespaceTree,
        _type_system: &TypeSystem,
        _trait_system: &TraitSystem,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement consistency checking between:
        // - Struct visibility and field visibility
        // - Trait visibility and method visibility
        // - Implementation visibility and method visibility
        // - Type parameter visibility constraints
    }

    /// Check for visibility violations in the namespace tree
    fn check_visibility_violations(
        &mut self,
        _namespace_tree: &NamespaceTree,
        _symbol_table: &SymbolTableBuilder,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement comprehensive visibility violation checking:
        // - Check all symbol references respect visibility rules
        // - Check trait implementations can access trait methods
        // - Check field access respects field visibility
        // - Check re-exports respect original visibility
    }

    /// Check access permission for a specific request
    #[allow(dead_code)]
    pub fn check_access(&mut self, request: &AccessRequest) -> AccessResult {
        // Check cache first
        if let Some(cached_result) = self.violation_cache.get(request) {
            return cached_result.clone();
        }

        // Perform access checking
        let result = self.perform_access_check(request);
        
        // Cache the result
        self.violation_cache.insert(request.clone(), result.clone());
        
        result
    }

    /// Perform the actual access checking logic
    fn perform_access_check(&self, request: &AccessRequest) -> AccessResult {
        let mut analysis = AccessAnalysis {
            visibility_chain: Vec::new(),
            applied_rules: Vec::new(),
            decision_context: Vec::new(),
        };

        // Step 1: Check definition-level visibility
        if let Some(visibility_rule) = self.visibility_table.definition_visibility.get(&request.target) {
            let step = VisibilityStep::DefinitionCheck {
                definition: request.target.clone(),
                visibility: visibility_rule.clone(),
                result: self.check_visibility_rule(visibility_rule, &request.accessor),
            };

            if let VisibilityStep::DefinitionCheck { result, .. } = &step {
                analysis.visibility_chain.push(step.clone());
                
                if !result {
                    return AccessResult {
                        permission: AccessPermission::Denied(AccessDenialReason::PrivateAccess),
                        analysis,
                        suggestions: self.generate_access_suggestions(request, AccessDenialReason::PrivateAccess),
                    };
                }
            }
        }

        // Step 2: Check bundle-level permissions
        let bundle_check_result = self.check_bundle_access(&request.target.bundle, &request.accessor.bundle);
        analysis.visibility_chain.push(VisibilityStep::BundleCheck {
            source_bundle: request.accessor.bundle.clone(),
            target_bundle: request.target.bundle.clone(),
            result: bundle_check_result,
        });

        if !bundle_check_result {
            return AccessResult {
                permission: AccessPermission::Denied(AccessDenialReason::BundleRestriction),
                analysis,
                suggestions: self.generate_access_suggestions(request, AccessDenialReason::BundleRestriction),
            };
        }

        // If all checks pass, grant access
        AccessResult {
            permission: AccessPermission::Granted,
            analysis,
            suggestions: Vec::new(),
        }
    }

    /// Check if a visibility rule allows access from an accessor
    fn check_visibility_rule(&self, rule: &VisibilityRule, accessor: &QualifiedName) -> bool {
        match rule {
            VisibilityRule::Private => {
                // Only accessible from the same namespace
                accessor.bundle == self.bundle_name && accessor.namespace == accessor.namespace
            }
            VisibilityRule::BundlePrivate => {
                // Accessible from the same bundle
                accessor.bundle == self.bundle_name
            }
            VisibilityRule::Public => {
                // Always accessible
                true
            }
            VisibilityRule::Restricted(allowed_bundles) => {
                // Accessible from specified bundles
                allowed_bundles.contains(&accessor.bundle)
            }
            VisibilityRule::Friend(friends) => {
                // Accessible from friend definitions
                friends.contains(accessor)
            }
        }
    }

    /// Check bundle-level access permissions
    fn check_bundle_access(&self, target_bundle: &BundleName, accessor_bundle: &BundleName) -> bool {
        // Same bundle access is always allowed
        if target_bundle == accessor_bundle {
            return true;
        }

        // Check bundle permissions
        if let Some(permissions) = self.visibility_table.bundle_permissions.get(target_bundle) {
            permissions.accessible_bundles.contains(accessor_bundle) ||
            permissions.authorized_accessors.contains(accessor_bundle)
        } else {
            // Default: allow access if no specific permissions are set
            true
        }
    }

    /// Generate suggestions for resolving access violations
    fn generate_access_suggestions(&self, request: &AccessRequest, reason: AccessDenialReason) -> Vec<AccessSuggestion> {
        let mut suggestions = Vec::new();

        match reason {
            AccessDenialReason::PrivateAccess => {
                suggestions.push(AccessSuggestion::MakePublic(request.target.clone()));
                suggestions.push(AccessSuggestion::ChangeVisibility(
                    request.target.clone(),
                    VisibilityRule::BundlePrivate
                ));
            }
            AccessDenialReason::BundleRestriction => {
                suggestions.push(AccessSuggestion::AddBundlePermission(request.accessor.bundle.clone()));
                suggestions.push(AccessSuggestion::ChangeVisibility(
                    request.target.clone(),
                    VisibilityRule::Public
                ));
            }
            AccessDenialReason::ScopeRestriction => {
                suggestions.push(AccessSuggestion::AddImport(format!("use {};", request.target.name)));
            }
            _ => {
                // Generic suggestions for other cases
                suggestions.push(AccessSuggestion::MakePublic(request.target.clone()));
            }
        }

        suggestions
    }

    /// Get visibility rule for a definition
    #[allow(dead_code)]
    pub fn get_definition_visibility(&self, qualified_name: &QualifiedName) -> Option<&VisibilityRule> {
        self.visibility_table.definition_visibility.get(qualified_name)
    }

    /// Get field visibility for a struct field
    #[allow(dead_code)]
    pub fn get_field_visibility(&self, struct_name: &QualifiedName, field_name: &str) -> Option<&VisibilityRule> {
        self.visibility_table.field_visibility.get(&(struct_name.clone(), field_name.to_string()))
    }

    /// Get method visibility for a method
    #[allow(dead_code)]
    pub fn get_method_visibility(&self, type_name: &QualifiedName, method_name: &str) -> Option<&VisibilityRule> {
        self.visibility_table.method_visibility.get(&(type_name.clone(), method_name.to_string()))
    }

    /// Check if a definition is publicly accessible
    #[allow(dead_code)]
    pub fn is_public(&self, qualified_name: &QualifiedName) -> bool {
        if let Some(rule) = self.get_definition_visibility(qualified_name) {
            matches!(rule, VisibilityRule::Public)
        } else {
            false
        }
    }

    /// Check if a definition is accessible from a specific bundle
    #[allow(dead_code)]
    pub fn is_accessible_from_bundle(&self, qualified_name: &QualifiedName, bundle: &BundleName) -> bool {
        if let Some(rule) = self.get_definition_visibility(qualified_name) {
            let accessor = QualifiedName {
                bundle: bundle.clone(),
                namespace: Vec::new(),
                name: String::new(),
            };
            self.check_visibility_rule(rule, &accessor)
        } else {
            false
        }
    }

    /// Get statistics about the visibility system
    #[allow(dead_code)]
    pub fn get_statistics(&self) -> VisibilityStatistics {
        let public_count = self.visibility_table.definition_visibility.values()
            .filter(|rule| matches!(rule, VisibilityRule::Public))
            .count();

        let private_count = self.visibility_table.definition_visibility.values()
            .filter(|rule| matches!(rule, VisibilityRule::Private))
            .count();

        let bundle_private_count = self.visibility_table.definition_visibility.values()
            .filter(|rule| matches!(rule, VisibilityRule::BundlePrivate))
            .count();

        VisibilityStatistics {
            total_definitions: self.visibility_table.definition_visibility.len(),
            public_definitions: public_count,
            private_definitions: private_count,
            bundle_private_definitions: bundle_private_count,
            field_visibility_rules: self.visibility_table.field_visibility.len(),
            method_visibility_rules: self.visibility_table.method_visibility.len(),
            namespace_exports: self.visibility_table.namespace_exports.len(),
            cached_access_checks: self.violation_cache.len(),
        }
    }
}

/// Statistics about the visibility system
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VisibilityStatistics {
    pub total_definitions: usize,
    pub public_definitions: usize,
    pub private_definitions: usize,
    pub bundle_private_definitions: usize,
    pub field_visibility_rules: usize,
    pub method_visibility_rules: usize,
    pub namespace_exports: usize,
    pub cached_access_checks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::Name;
    use crate::lexical::token::Position;

    fn create_test_qualified_name(bundle: &str, namespace: Vec<&str>, name: &str) -> QualifiedName {
        QualifiedName {
            bundle: BundleName::from(bundle),
            namespace: namespace.into_iter().map(String::from).collect(),
            name: name.to_string(),
        }
    }

    fn create_test_access_request(target: QualifiedName, accessor: QualifiedName, kind: AccessKind) -> AccessRequest {
        AccessRequest {
            target,
            accessor,
            access_kind: kind,
        }
    }

    #[test]
    fn test_visibility_system_creation() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name.clone());
        
        assert_eq!(visibility_system.bundle_name, bundle_name);
        assert!(visibility_system.visibility_table.definition_visibility.is_empty());
    }

    #[test]
    fn test_visibility_rule_validation() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name);
        
        let qualified_name = create_test_qualified_name("test", vec![], "symbol");
        let def = Definition {
            decorators: Vec::new(),
            visibility: None,
            name: Name {
                value: "symbol".to_string(),
                span: Span::single(Position::new_start()),
            },
            type_spec: None,
            expr: DefExpr::Exp(crate::syntax::ast::Exp {
                kind: crate::syntax::ast::ExpKind::Nil,
                span: Span::single(Position::new_start()),
            }),
            span: Span::single(Position::new_start()),
        };

        // Test valid public rule
        let public_rule = VisibilityRule::Public;
        assert!(visibility_system.validate_visibility_rule(&public_rule, &qualified_name, &def).is_ok());

        // Test empty restricted rule (should fail)
        let empty_restricted = VisibilityRule::Restricted(Vec::new());
        assert!(visibility_system.validate_visibility_rule(&empty_restricted, &qualified_name, &def).is_err());

        // Test valid restricted rule
        let valid_restricted = VisibilityRule::Restricted(vec![BundleName::from("allowed")]);
        assert!(visibility_system.validate_visibility_rule(&valid_restricted, &qualified_name, &def).is_ok());
    }

    #[test]
    fn test_visibility_rule_checking() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name);
        
        let accessor = create_test_qualified_name("test", vec![], "accessor");
        let external_accessor = create_test_qualified_name("other", vec![], "accessor");

        // Test public visibility
        assert!(visibility_system.check_visibility_rule(&VisibilityRule::Public, &accessor));
        assert!(visibility_system.check_visibility_rule(&VisibilityRule::Public, &external_accessor));

        // Test bundle-private visibility
        assert!(visibility_system.check_visibility_rule(&VisibilityRule::BundlePrivate, &accessor));
        assert!(!visibility_system.check_visibility_rule(&VisibilityRule::BundlePrivate, &external_accessor));

        // Test restricted visibility
        let restricted_rule = VisibilityRule::Restricted(vec![BundleName::from("other")]);
        assert!(!visibility_system.check_visibility_rule(&restricted_rule, &accessor));
        assert!(visibility_system.check_visibility_rule(&restricted_rule, &external_accessor));
    }

    #[test]
    fn test_access_request_creation() {
        let target = create_test_qualified_name("target_bundle", vec!["ns"], "symbol");
        let accessor = create_test_qualified_name("accessor_bundle", vec![], "accessor");
        
        let request = create_test_access_request(target.clone(), accessor.clone(), AccessKind::TypeAccess);
        
        assert_eq!(request.target, target);
        assert_eq!(request.accessor, accessor);
        assert_eq!(request.access_kind, AccessKind::TypeAccess);
    }

    #[test]
    fn test_bundle_access_checking() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name.clone());
        
        // Same bundle access should always be allowed
        assert!(visibility_system.check_bundle_access(&bundle_name, &bundle_name));
        
        // Different bundle access depends on permissions (default true for now)
        let other_bundle = BundleName::from("other");
        assert!(visibility_system.check_bundle_access(&other_bundle, &bundle_name));
    }

    #[test]
    fn test_visibility_statistics() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name);
        
        let stats = visibility_system.get_statistics();
        assert_eq!(stats.total_definitions, 0);
        assert_eq!(stats.public_definitions, 0);
        assert_eq!(stats.private_definitions, 0);
        assert_eq!(stats.bundle_private_definitions, 0);
    }

    #[test]
    fn test_access_suggestion_generation() {
        let bundle_name = BundleName::from("test");
        let visibility_system = VisibilitySystem::new(bundle_name);
        
        let request = create_test_access_request(
            create_test_qualified_name("test", vec![], "target"),
            create_test_qualified_name("test", vec![], "accessor"),
            AccessKind::ValueAccess,
        );

        let suggestions = visibility_system.generate_access_suggestions(&request, AccessDenialReason::PrivateAccess);
        assert!(!suggestions.is_empty());
        
        // Should suggest making the target public
        assert!(suggestions.iter().any(|s| matches!(s, AccessSuggestion::MakePublic(_))));
    }
}