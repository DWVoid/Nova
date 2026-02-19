//! Nova Trait System Implementation
//!
//! This module implements Nova's trait system including trait definitions,
//! implementations, coherence checking, and method resolution. It provides
//! the foundation for Nova's advanced type system features and polymorphism.

use crate::syntax::ast::{TopItem, Definition, DefExpr, TraitDef, Implementation, TraitSig};
use crate::lexical::token::Span;
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName};
use super::bundle::BundleName;
use super::namespace::{NamespaceTree, NamespacePath};
use super::types::{TypeSystem, NovaType, Parameter};
use super::symbols::SymbolTableBuilder;
use std::collections::{HashMap, HashSet};

/// Core trait system manager
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitSystem {
    /// Current bundle being analyzed
    bundle_name: BundleName,
    /// Trait definitions indexed by qualified name
    trait_definitions: HashMap<QualifiedName, TraitDefinition>,
    /// Implementation table tracking all implementations
    implementation_table: ImplementationTable,
    /// Coherence graph for tracking implementation relationships
    coherence_graph: CoherenceGraph,
    /// Method resolution cache
    method_resolution_cache: HashMap<(NovaType, String), MethodResolution>,
}

/// Trait definition in the semantic model
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitDefinition {
    /// Qualified name of the trait
    pub name: QualifiedName,
    /// Method signatures defined by this trait
    pub signatures: HashMap<String, TraitMethodSignature>,
    /// Type parameters of the trait
    pub type_parameters: Vec<TraitTypeParameter>,
    /// Super traits that this trait extends
    pub super_traits: Vec<TraitConstraint>,
    /// Visibility of the trait
    pub visibility: super::Visibility,
    /// Source location
    pub span: Span,
}

/// Method signature within a trait
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitMethodSignature {
    /// Method name
    pub name: String,
    /// Type parameters specific to this method
    pub type_parameters: Vec<TraitTypeParameter>,
    /// Method parameters
    pub parameters: Vec<Parameter>,
    /// Return type
    pub return_type: NovaType,
    /// Additional constraints on this method
    pub constraints: Vec<TypeConstraint>,
    /// Whether this method is const
    pub is_const: bool,
    /// Default implementation if any
    pub default_implementation: Option<FunctionBody>,
    /// Source location
    pub span: Span,
}

/// Type parameter in trait definitions
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitTypeParameter {
    /// Parameter name
    pub name: String,
    /// Trait bounds on this parameter
    pub bounds: Vec<TraitConstraint>,
    /// Default type if any
    pub default_type: Option<NovaType>,
    /// Variance (covariant, contravariant, invariant)
    pub variance: TypeVariance,
}

/// Variance of type parameters
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TypeVariance {
    /// Type parameter is covariant (can be substituted with subtype)
    Covariant,
    /// Type parameter is contravariant (can be substituted with supertype)
    Contravariant,
    /// Type parameter is invariant (exact type match required)
    Invariant,
}

/// Constraint on types in trait context
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TypeConstraint {
    /// Type must implement the specified trait
    TraitBound(NovaType, QualifiedName),
    /// Types must be equal
    Equality(NovaType, NovaType),
    /// Type must be a subtype of another
    Subtype(NovaType, NovaType),
    /// Type must have lifetime at least as long as another
    Lifetime(String, String),
}

/// Trait constraint specification
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub struct TraitConstraint {
    /// The trait that must be implemented
    pub trait_name: QualifiedName,
    /// Type arguments to the trait
    pub type_arguments: Vec<NovaType>,
}

/// Function body representation for default implementations
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum FunctionBody {
    /// Lambda-style body with block
    Block(crate::syntax::ast::Block),
    /// External function reference
    External(String),
}

/// Implementation table tracking all trait implementations
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct ImplementationTable {
    /// Trait implementations: (Type, Trait) -> Implementation
    pub trait_implementations: HashMap<(NovaType, QualifiedName), TraitImplementation>,
    /// Inherent implementations: Type -> [Implementation]
    pub inherent_implementations: HashMap<NovaType, Vec<InherentImplementation>>,
    /// Implementation conflicts detected during coherence checking
    pub implementation_conflicts: Vec<ImplementationConflict>,
}

/// Implementation of a trait for a specific type
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitImplementation {
    /// The type implementing the trait
    pub implementing_type: NovaType,
    /// The trait being implemented
    pub trait_name: QualifiedName,
    /// Type arguments to the trait if generic
    pub trait_type_arguments: Vec<NovaType>,
    /// Method implementations
    pub method_implementations: HashMap<String, MethodImplementation>,
    /// Associated type implementations
    pub associated_types: HashMap<String, NovaType>,
    /// Implementation constraints
    pub constraints: Vec<TypeConstraint>,
    /// Whether this is a coherent implementation
    pub is_coherent: bool,
    /// Source location
    pub span: Span,
}

/// Implementation of methods directly on a type (not through traits)
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct InherentImplementation {
    /// The type receiving the implementation
    pub target_type: NovaType,
    /// Method implementations
    pub method_implementations: HashMap<String, MethodImplementation>,
    /// Source location
    pub span: Span,
}

/// Implementation of a specific method
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct MethodImplementation {
    /// Method signature
    pub signature: TraitMethodSignature,
    /// Method body
    pub body: FunctionBody,
    /// Whether this overrides a default implementation
    pub is_override: bool,
    /// Source location
    pub span: Span,
}

/// Coherence graph for tracking implementation relationships
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct CoherenceGraph {
    /// Nodes in the coherence graph: (Type, Trait) pairs
    pub nodes: HashSet<(NovaType, QualifiedName)>,
    /// Edges representing implementation relationships
    pub edges: HashSet<((NovaType, QualifiedName), (NovaType, QualifiedName))>,
    /// Coherence violations detected
    pub violations: Vec<CoherenceViolation>,
}

/// Violation of coherence rules
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct CoherenceViolation {
    /// Description of the violation
    pub message: String,
    /// Conflicting implementations
    pub conflicting_implementations: Vec<(NovaType, QualifiedName)>,
    /// Source locations involved
    pub locations: Vec<Span>,
}

/// Conflict between implementations
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ImplementationConflict {
    /// The type and trait involved in the conflict
    pub target: (NovaType, QualifiedName),
    /// Conflicting implementations
    pub implementations: Vec<TraitImplementation>,
    /// Reason for the conflict
    pub conflict_reason: ConflictReason,
    /// Source locations
    pub locations: Vec<Span>,
}

/// Reason for implementation conflict
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ConflictReason {
    /// Multiple implementations of the same trait for the same type
    DuplicateImplementation,
    /// Overlapping implementations (coherence violation)
    OverlappingImplementation,
    /// Orphan rule violation
    OrphanRuleViolation,
    /// Implementation doesn't match trait definition
    SignatureMismatch,
}

/// Method resolution result
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct MethodResolution {
    /// Candidates found during resolution
    pub candidates: Vec<MethodCandidate>,
    /// Selected method after resolution
    pub selected_method: Option<ResolvedMethod>,
    /// Resolution steps taken
    pub resolution_steps: Vec<ResolutionStep>,
}

/// Candidate method during resolution
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum MethodCandidate {
    /// Method from inherent implementation
    Inherent {
        target_type: NovaType,
        method: MethodImplementation,
    },
    /// Method from trait implementation
    TraitMethod {
        target_type: NovaType,
        trait_name: QualifiedName,
        method: MethodImplementation,
    },
    /// Default method from trait definition
    DefaultMethod {
        trait_name: QualifiedName,
        method: TraitMethodSignature,
    },
}

/// Resolved method after selection
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ResolvedMethod {
    /// The selected method
    pub method: MethodCandidate,
    /// Type substitutions made during resolution
    pub type_substitutions: HashMap<String, NovaType>,
    /// Trait constraints that must be satisfied
    pub required_constraints: Vec<TypeConstraint>,
}

/// Step in method resolution process
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum ResolutionStep {
    /// Collected candidates from inherent implementations
    InherentCandidates(Vec<MethodCandidate>),
    /// Collected candidates from trait implementations
    TraitCandidates(Vec<MethodCandidate>),
    /// Applied type unification
    TypeUnification(NovaType, NovaType),
    /// Checked trait bounds
    TraitBoundCheck(NovaType, QualifiedName),
    /// Selected final method
    MethodSelection(ResolvedMethod),
}

impl TraitSystem {
    /// Create a new trait system for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        Self {
            bundle_name,
            trait_definitions: HashMap::new(),
            implementation_table: ImplementationTable::default(),
            coherence_graph: CoherenceGraph::default(),
            method_resolution_cache: HashMap::new(),
        }
    }

    /// Build trait system from namespace tree and type system
    pub fn build_from_namespace_tree(
        &mut self,
        namespace_tree: &NamespaceTree,
        type_system: &TypeSystem,
        symbol_table: &SymbolTableBuilder,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Phase 1: Collect trait definitions
        self.collect_trait_definitions(namespace_tree, diagnostics);

        // Phase 2: Process implementation blocks
        self.process_implementations(namespace_tree, type_system, diagnostics);

        // Phase 3: Build coherence graph
        self.build_coherence_graph(diagnostics);

        // Phase 4: Validate coherence and detect conflicts
        self.validate_coherence(diagnostics);

        // Phase 5: Build method resolution tables
        self.build_method_resolution_tables(type_system, symbol_table, diagnostics);
    }

    /// Collect trait definitions from namespace tree
    fn collect_trait_definitions(
        &mut self,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for (symbol_name, local_def) in &namespace_scope.definitions {
                if let TopItem::Definition(def) = &local_def.item {
                    if let DefExpr::Trait(trait_def) = &def.expr {
                        let qualified_name = QualifiedName {
                            bundle: namespace_tree.root.clone(),
                            namespace: namespace_path.0.clone(),
                            name: symbol_name.clone(),
                        };

                        match self.extract_trait_definition(trait_def, &qualified_name, def) {
                            Ok(trait_definition) => {
                                // Check for duplicate trait definitions
                                if self.trait_definitions.contains_key(&qualified_name) {
                                    diagnostics.push(SemanticDiagnostic {
                                        severity: DiagnosticSeverity::Error,
                                        message: format!(
                                            "Duplicate trait definition: '{}' in namespace '{}'",
                                            symbol_name, namespace_path
                                        ),
                                location: def.span.start,
                                category: DiagnosticCategory::TypeError,
                                    });
                                } else {
                                    self.trait_definitions.insert(qualified_name, trait_definition);
                                }
                            }
                            Err(err) => diagnostics.push(err),
                        }
                    }
                }
            }
        }
    }

    /// Extract trait definition from AST
    fn extract_trait_definition(
        &self,
        trait_def: &TraitDef,
        qualified_name: &QualifiedName,
        def: &Definition,
    ) -> Result<TraitDefinition, SemanticDiagnostic> {
        let mut signatures = HashMap::new();

        // Extract method signatures from trait
        for sig in &trait_def.sigs {
            let method_signature = self.extract_method_signature(sig)?;
            
            // Check for duplicate method names
            if signatures.contains_key(&method_signature.name) {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Duplicate method '{}' in trait '{}'",
                        method_signature.name, qualified_name.name
                    ),
                    location: sig.span.start,
                    category: DiagnosticCategory::TypeError,
                });
            }

            signatures.insert(method_signature.name.clone(), method_signature);
        }

        let visibility = if def.visibility.is_some() {
            super::Visibility::Public
        } else {
            super::Visibility::Private
        };

        Ok(TraitDefinition {
            name: qualified_name.clone(),
            signatures,
            type_parameters: Vec::new(), // TODO: Extract type parameters
            super_traits: Vec::new(),     // TODO: Extract super traits
            visibility,
            span: def.span,
        })
    }

    /// Extract method signature from trait signature AST
    fn extract_method_signature(&self, sig: &TraitSig) -> Result<TraitMethodSignature, SemanticDiagnostic> {
        let mut parameters = Vec::new();

        // Convert AST parameters to semantic parameters
        for param in &sig.params {
            if let Some(type_spec) = &param.type_spec {
                let param_type = self.convert_ast_type_to_nova_type(&type_spec.ty);
                parameters.push(Parameter {
                    name: param.name.value.clone(),
                    param_type,
                    is_mutable: false, // TODO: Extract mutability
                });
            } else {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Method parameter '{}' must have explicit type in trait definition",
                        param.name.value
                    ),
                    location: param.name.span.start,
                    category: DiagnosticCategory::TypeError,
                });
            }
        }

        let return_type = self.convert_ast_type_to_nova_type(&sig.return_type.ty);

        Ok(TraitMethodSignature {
            name: sig.name.value.clone(),
            type_parameters: Vec::new(), // TODO: Extract method type parameters
            parameters,
            return_type,
            constraints: Vec::new(), // TODO: Extract constraints
            is_const: false,         // TODO: Extract const modifier
            default_implementation: None, // TODO: Handle default implementations
            span: sig.span,
        })
    }

    /// Convert AST type to Nova type (simplified version)
    fn convert_ast_type_to_nova_type(&self, type_name: &crate::syntax::ast::TypeName) -> NovaType {
        if type_name.parts.len() == 1 {
            let type_str = &type_name.parts[0].value;
            
            // Handle primitive types
            match type_str.as_str() {
                "integer" => NovaType::Primitive(super::types::PrimitiveType::Integer),
                "float" => NovaType::Primitive(super::types::PrimitiveType::Float),
                "boolean" => NovaType::Primitive(super::types::PrimitiveType::Boolean),
                "string" => NovaType::Primitive(super::types::PrimitiveType::String),
                "unit" => NovaType::Primitive(super::types::PrimitiveType::Unit),
                _ => {
                    // Handle named types
                    let qualified_name = QualifiedName {
                        bundle: self.bundle_name.clone(),
                        namespace: Vec::new(),
                        name: type_str.clone(),
                    };
                    NovaType::Named(qualified_name, Vec::new())
                }
            }
        } else {
            // Handle qualified type names
            let qualified_name = QualifiedName {
                bundle: self.bundle_name.clone(),
                namespace: type_name.parts.iter().take(type_name.parts.len().saturating_sub(1))
                    .map(|n| n.value.clone()).collect(),
                name: type_name.parts.last().unwrap().value.clone(),
            };
            NovaType::Named(qualified_name, Vec::new())
        }
    }

    /// Process implementation blocks
    fn process_implementations(
        &mut self,
        namespace_tree: &NamespaceTree,
        type_system: &TypeSystem,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for implementation in &namespace_scope.implementations {
                self.process_implementation_block(
                    &implementation.implementation,
                    namespace_path,
                    type_system,
                    diagnostics,
                );
            }
        }
    }

    /// Process a single implementation block
    fn process_implementation_block(
        &mut self,
        impl_block: &Implementation,
        namespace_path: &NamespacePath,
        _type_system: &TypeSystem,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        let target_type = self.convert_ast_type_to_nova_type(&impl_block.target);

        match &impl_block.trait_type {
            Some(trait_type_name) => {
                // Trait implementation
                let trait_name = QualifiedName {
                    bundle: self.bundle_name.clone(),
                    namespace: if trait_type_name.parts.len() > 1 {
                        trait_type_name.parts.iter().take(trait_type_name.parts.len().saturating_sub(1))
                            .map(|n| n.value.clone()).collect()
                    } else {
                        namespace_path.0.clone()
                    },
                    name: trait_type_name.parts.last().unwrap().value.clone(),
                };

                match self.create_trait_implementation(impl_block, target_type, trait_name, _type_system) {
                    Ok(trait_impl) => {
                        let key = (trait_impl.implementing_type.clone(), trait_impl.trait_name.clone());
                        
                        // Check for duplicate implementations
                        if self.implementation_table.trait_implementations.contains_key(&key) {
                            diagnostics.push(SemanticDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Duplicate implementation of trait '{}' for type '{}'",
                                    trait_impl.trait_name.name,
                                    format!("{:?}", trait_impl.implementing_type) // TODO: Better type display
                                ),
                                location: impl_block.span.start,
                                category: DiagnosticCategory::TypeError,
                            });
                        } else {
                            self.implementation_table.trait_implementations.insert(key, trait_impl);
                        }
                    }
                    Err(err) => diagnostics.push(err),
                }
            }
            None => {
                // Inherent implementation
                match self.create_inherent_implementation(impl_block, target_type, _type_system) {
                    Ok(inherent_impl) => {
                        self.implementation_table.inherent_implementations
                            .entry(inherent_impl.target_type.clone())
                            .or_insert_with(Vec::new)
                            .push(inherent_impl);
                    }
                    Err(err) => diagnostics.push(err),
                }
            }
        }
    }

    /// Create trait implementation from AST
    fn create_trait_implementation(
        &self,
        impl_block: &Implementation,
        target_type: NovaType,
        trait_name: QualifiedName,
        _type_system: &TypeSystem,
    ) -> Result<TraitImplementation, SemanticDiagnostic> {
        // Check if trait exists
        if !self.trait_definitions.contains_key(&trait_name) {
            return Err(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Trait '{}' not found", trait_name.name),
                location: impl_block.span.start,
                category: DiagnosticCategory::TypeError,
            });
        }

        let trait_def = self.trait_definitions.get(&trait_name).unwrap();
        let mut method_implementations = HashMap::new();

        // Process method implementations
        for item in &impl_block.items {
            if let Definition { expr: DefExpr::Exp(exp), .. } = item {
                if let crate::syntax::ast::ExpKind::Lambda(lambda) = &exp.kind {
                    // Check if this method exists in trait
                    if let Some(trait_method) = trait_def.signatures.get(&item.name.value) {
                        let method_impl = self.create_method_implementation(item, lambda, trait_method)?;
                        method_implementations.insert(item.name.value.clone(), method_impl);
                    } else {
                        return Err(SemanticDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!(
                                "Method '{}' not declared in trait '{}'",
                                item.name.value, trait_name.name
                            ),
                            location: item.name.span.start,
                            category: DiagnosticCategory::TypeError,
                        });
                    }
                }
            }
        }

        // Check that all required methods are implemented
        for (method_name, trait_method) in &trait_def.signatures {
            if !method_implementations.contains_key(method_name) && trait_method.default_implementation.is_none() {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Missing implementation for method '{}' in trait '{}'",
                        method_name, trait_name.name
                    ),
                    location: impl_block.span.start,
                    category: DiagnosticCategory::TypeError,
                });
            }
        }

        Ok(TraitImplementation {
            implementing_type: target_type,
            trait_name,
            trait_type_arguments: Vec::new(), // TODO: Extract type arguments
            method_implementations,
            associated_types: HashMap::new(), // TODO: Handle associated types
            constraints: Vec::new(),          // TODO: Extract constraints
            is_coherent: true,               // Will be validated later
            span: impl_block.span,
        })
    }

    /// Create inherent implementation from AST
    fn create_inherent_implementation(
        &self,
        impl_block: &Implementation,
        target_type: NovaType,
        _type_system: &TypeSystem,
    ) -> Result<InherentImplementation, SemanticDiagnostic> {
        let mut method_implementations = HashMap::new();

        // Process method implementations
        for item in &impl_block.items {
            if let Definition { expr: DefExpr::Exp(exp), .. } = item {
                if let crate::syntax::ast::ExpKind::Lambda(lambda) = &exp.kind {
                    // For inherent implementations, we create the signature from the lambda
                    let trait_method = self.create_trait_method_from_lambda(item, lambda)?;
                    let method_impl = self.create_method_implementation(item, lambda, &trait_method)?;
                    method_implementations.insert(item.name.value.clone(), method_impl);
                }
            }
        }

        Ok(InherentImplementation {
            target_type,
            method_implementations,
            span: impl_block.span,
        })
    }

    /// Create trait method signature from lambda (for inherent implementations)
    fn create_trait_method_from_lambda(
        &self,
        def: &Definition,
        lambda: &crate::syntax::ast::LambdaExpr,
    ) -> Result<TraitMethodSignature, SemanticDiagnostic> {
        let mut parameters = Vec::new();

        for param in &lambda.params {
            if let Some(type_spec) = &param.type_spec {
                let param_type = self.convert_ast_type_to_nova_type(&type_spec.ty);
                parameters.push(Parameter {
                    name: param.name.value.clone(),
                    param_type,
                    is_mutable: false, // TODO: Extract mutability
                });
            } else {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Method parameter '{}' must have explicit type",
                        param.name.value
                    ),
                    location: param.name.span.start,
                    category: DiagnosticCategory::TypeError,
                });
            }
        }

        let return_type = self.convert_ast_type_to_nova_type(&lambda.return_type.ty);

        Ok(TraitMethodSignature {
            name: def.name.value.clone(),
            type_parameters: Vec::new(),
            parameters,
            return_type,
            constraints: Vec::new(),
            is_const: lambda.is_const,
            default_implementation: None,
            span: def.span,
        })
    }

    /// Create method implementation from lambda
    fn create_method_implementation(
        &self,
        def: &Definition,
        lambda: &crate::syntax::ast::LambdaExpr,
        trait_method: &TraitMethodSignature,
    ) -> Result<MethodImplementation, SemanticDiagnostic> {
        // Validate signature compatibility
        if lambda.params.len() != trait_method.parameters.len() {
            return Err(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "Method '{}' parameter count mismatch: expected {}, found {}",
                    def.name.value,
                    trait_method.parameters.len(),
                    lambda.params.len()
                ),
                location: def.name.span.start,
                category: DiagnosticCategory::TypeError,
            });
        }

        // TODO: Validate parameter types and return type compatibility

        Ok(MethodImplementation {
            signature: trait_method.clone(),
            body: FunctionBody::Block(lambda.block.clone()),
            is_override: false, // TODO: Determine override status
            span: def.span,
        })
    }

    /// Build coherence graph from implementations
    fn build_coherence_graph(&mut self, _diagnostics: &mut Vec<SemanticDiagnostic>) {
        // Add all trait implementations as nodes
        for (type_trait_pair, _) in &self.implementation_table.trait_implementations {
            self.coherence_graph.nodes.insert(type_trait_pair.clone());
        }

        // TODO: Build edges based on trait relationships and type relationships
        // TODO: Detect potential coherence violations

        // For now, we'll do a basic check
        let mut seen_implementations: HashMap<QualifiedName, Vec<NovaType>> = HashMap::new();

        for (type_trait_pair, _implementation) in &self.implementation_table.trait_implementations {
            let trait_name = &type_trait_pair.1;
            let implementing_type = &type_trait_pair.0;

            seen_implementations.entry(trait_name.clone())
                .or_insert_with(Vec::new)
                .push(implementing_type.clone());
        }

        // Check for obvious conflicts (same trait for same type multiple times)
        // More sophisticated coherence checking would go here
    }

    /// Validate coherence and detect conflicts
    fn validate_coherence(&mut self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        // Check for orphan rule violations
        self.check_orphan_rule(diagnostics);

        // Check for overlapping implementations
        self.check_overlapping_implementations(diagnostics);

        // Check for completeness of implementations
        self.check_implementation_completeness(diagnostics);
    }

    /// Check orphan rule: implementations must be in same bundle as either type or trait
    fn check_orphan_rule(&mut self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        let mut violations = Vec::new();
        
        for ((implementing_type, trait_name), implementation) in &self.implementation_table.trait_implementations {
            let type_bundle = match implementing_type {
                NovaType::Named(qualified_name, _) => &qualified_name.bundle,
                _ => &self.bundle_name, // Primitive types belong to current bundle for the purpose of orphan rule
            };

            let trait_bundle = &trait_name.bundle;
            let impl_bundle = &self.bundle_name;

            if impl_bundle != type_bundle && impl_bundle != trait_bundle {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Orphan rule violation: implementation of trait '{}' for type '{}' must be in same bundle as either trait or type",
                        trait_name.name,
                        format!("{:?}", implementing_type) // TODO: Better type display
                    ),
                    location: implementation.span.start,
                    category: DiagnosticCategory::TypeError,
                });

                // Collect for later marking as not coherent
                violations.push((implementing_type.clone(), trait_name.clone()));
            }
        }

        // Mark violations as not coherent
        for (implementing_type, trait_name) in violations {
            if let Some(implementation) = self.implementation_table.trait_implementations.get_mut(&(implementing_type, trait_name)) {
                implementation.is_coherent = false;
            }
        }
    }

    /// Check for overlapping implementations
    fn check_overlapping_implementations(&mut self, _diagnostics: &mut Vec<SemanticDiagnostic>) {
        // TODO: Implement sophisticated overlapping implementation detection
        // For now, we'll do basic duplicate detection which is already handled during collection
    }

    /// Check implementation completeness
    fn check_implementation_completeness(&mut self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        for ((implementing_type, trait_name), implementation) in &self.implementation_table.trait_implementations {
            if let Some(trait_def) = self.trait_definitions.get(trait_name) {
                // Check that all required methods are implemented
                for (method_name, trait_method) in &trait_def.signatures {
                    if !implementation.method_implementations.contains_key(method_name) 
                        && trait_method.default_implementation.is_none() {
                        diagnostics.push(SemanticDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!(
                                "Incomplete implementation: missing method '{}' for trait '{}' on type '{}'",
                                method_name,
                                trait_name.name,
                                format!("{:?}", implementing_type) // TODO: Better type display
                            ),
                            location: implementation.span.start,
                            category: DiagnosticCategory::TypeError,
                        });
                    }
                }
            }
        }
    }

    /// Build method resolution tables
    fn build_method_resolution_tables(
        &mut self,
        _type_system: &TypeSystem,
        _symbol_table: &SymbolTableBuilder,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Pre-compute common method resolutions
        // For now, method resolution will be done on-demand via resolve_method
    }

    /// Resolve method call for a given type and method name
    #[allow(dead_code)]
    pub fn resolve_method(
        &mut self,
        target_type: &NovaType,
        method_name: &str,
        type_arguments: &[NovaType],
    ) -> MethodResolution {
        let cache_key = (target_type.clone(), method_name.to_string());
        
        // Check cache first
        if let Some(cached_result) = self.method_resolution_cache.get(&cache_key) {
            return cached_result.clone();
        }

        let mut resolution = MethodResolution {
            candidates: Vec::new(),
            selected_method: None,
            resolution_steps: Vec::new(),
        };

        // Step 1: Collect inherent method candidates
        let inherent_candidates = self.collect_inherent_candidates(target_type, method_name);
        resolution.candidates.extend(inherent_candidates.clone());
        resolution.resolution_steps.push(ResolutionStep::InherentCandidates(inherent_candidates));

        // Step 2: Collect trait method candidates
        let trait_candidates = self.collect_trait_candidates(target_type, method_name);
        resolution.candidates.extend(trait_candidates.clone());
        resolution.resolution_steps.push(ResolutionStep::TraitCandidates(trait_candidates));

        // Step 3: Select best candidate
        if let Some(selected) = self.select_best_candidate(&resolution.candidates, target_type, type_arguments) {
            resolution.selected_method = Some(selected.clone());
            resolution.resolution_steps.push(ResolutionStep::MethodSelection(selected));
        }

        // Cache the result
        self.method_resolution_cache.insert(cache_key, resolution.clone());
        resolution
    }

    /// Collect inherent method candidates
    fn collect_inherent_candidates(&self, target_type: &NovaType, method_name: &str) -> Vec<MethodCandidate> {
        let mut candidates = Vec::new();

        if let Some(implementations) = self.implementation_table.inherent_implementations.get(target_type) {
            for implementation in implementations {
                if let Some(method) = implementation.method_implementations.get(method_name) {
                    candidates.push(MethodCandidate::Inherent {
                        target_type: target_type.clone(),
                        method: method.clone(),
                    });
                }
            }
        }

        candidates
    }

    /// Collect trait method candidates
    fn collect_trait_candidates(&self, target_type: &NovaType, method_name: &str) -> Vec<MethodCandidate> {
        let mut candidates = Vec::new();

        // Look through all trait implementations for this type
        for ((impl_type, trait_name), implementation) in &self.implementation_table.trait_implementations {
            if self.types_compatible(target_type, impl_type) {
                if let Some(method) = implementation.method_implementations.get(method_name) {
                    candidates.push(MethodCandidate::TraitMethod {
                        target_type: target_type.clone(),
                        trait_name: trait_name.clone(),
                        method: method.clone(),
                    });
                }
            }
        }

        candidates
    }

    /// Check if two types are compatible for method resolution
    fn types_compatible(&self, type1: &NovaType, type2: &NovaType) -> bool {
        // TODO: Implement proper type compatibility checking including subtyping
        type1 == type2
    }

    /// Select best candidate from available options
    fn select_best_candidate(
        &self,
        candidates: &[MethodCandidate],
        target_type: &NovaType,
        _type_arguments: &[NovaType],
    ) -> Option<ResolvedMethod> {
        if candidates.is_empty() {
            return None;
        }

        // Priority rules:
        // 1. Inherent methods take precedence over trait methods
        // 2. More specific implementations take precedence over generic ones
        // 3. Methods with exact type matches take precedence

        for candidate in candidates {
            match candidate {
                MethodCandidate::Inherent { target_type: candidate_type, method: _ } => {
                    if self.types_compatible(target_type, candidate_type) {
                        return Some(ResolvedMethod {
                            method: candidate.clone(),
                            type_substitutions: HashMap::new(), // TODO: Calculate substitutions
                            required_constraints: Vec::new(),    // TODO: Calculate constraints
                        });
                    }
                }
                MethodCandidate::TraitMethod { target_type: candidate_type, trait_name: _, method: _ } => {
                    if self.types_compatible(target_type, candidate_type) {
                        return Some(ResolvedMethod {
                            method: candidate.clone(),
                            type_substitutions: HashMap::new(), // TODO: Calculate substitutions
                            required_constraints: Vec::new(),    // TODO: Calculate constraints
                        });
                    }
                }
                MethodCandidate::DefaultMethod { trait_name, method: _ } => {
                    // Check if target type implements this trait
                    let key = (target_type.clone(), trait_name.clone());
                    if self.implementation_table.trait_implementations.contains_key(&key) {
                        return Some(ResolvedMethod {
                            method: candidate.clone(),
                            type_substitutions: HashMap::new(),
                            required_constraints: Vec::new(),
                        });
                    }
                }
            }
        }

        None
    }

    /// Get trait definition by qualified name
    #[allow(dead_code)]
    pub fn get_trait_definition(&self, qualified_name: &QualifiedName) -> Option<&TraitDefinition> {
        self.trait_definitions.get(qualified_name)
    }

    /// Check if a type implements a trait
    #[allow(dead_code)]
    pub fn type_implements_trait(&self, type_: &NovaType, trait_name: &QualifiedName) -> bool {
        let key = (type_.clone(), trait_name.clone());
        self.implementation_table.trait_implementations.contains_key(&key)
    }

    /// Get all trait implementations for a type
    #[allow(dead_code)]
    pub fn get_trait_implementations_for_type(&self, type_: &NovaType) -> Vec<&TraitImplementation> {
        self.implementation_table.trait_implementations
            .iter()
            .filter(|((impl_type, _), _)| self.types_compatible(type_, impl_type))
            .map(|(_, implementation)| implementation)
            .collect()
    }

    /// Get statistics about the trait system
    #[allow(dead_code)]
    pub fn get_statistics(&self) -> TraitSystemStatistics {
        TraitSystemStatistics {
            trait_count: self.trait_definitions.len(),
            trait_implementation_count: self.implementation_table.trait_implementations.len(),
            inherent_implementation_count: self.implementation_table.inherent_implementations.len(),
            coherence_violation_count: self.coherence_graph.violations.len(),
            method_resolution_cache_size: self.method_resolution_cache.len(),
        }
    }
}

/// Statistics about the trait system
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TraitSystemStatistics {
    pub trait_count: usize,
    pub trait_implementation_count: usize,
    pub inherent_implementation_count: usize,
    pub coherence_violation_count: usize,
    pub method_resolution_cache_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::{Name, TypeName};
    use crate::lexical::token::Position;
    use crate::semantic::types::PrimitiveType;

    #[allow(dead_code)]
    fn create_test_qualified_name(name: &str) -> QualifiedName {
        QualifiedName {
            bundle: BundleName::from("test"),
            namespace: Vec::new(),
            name: name.to_string(),
        }
    }

    fn create_test_type_name(name: &str) -> TypeName {
        TypeName {
            span: Span::single(Position::start()),
            parts: vec![Name {
                value: name.to_string(),
                span: Span::single(Position::start()),
            }],
        }
    }

    #[test]
    fn test_trait_system_creation() {
        let bundle_name = BundleName::from("test");
        let trait_system = TraitSystem::new(bundle_name.clone());
        
        assert_eq!(trait_system.bundle_name, bundle_name);
        assert!(trait_system.trait_definitions.is_empty());
        assert!(trait_system.implementation_table.trait_implementations.is_empty());
    }

    #[test]
    fn test_ast_type_conversion() {
        let bundle_name = BundleName::from("test");
        let trait_system = TraitSystem::new(bundle_name);
        
        let type_name = create_test_type_name("integer");
        let nova_type = trait_system.convert_ast_type_to_nova_type(&type_name);
        
        assert_eq!(nova_type, NovaType::Primitive(PrimitiveType::Integer));
    }

    #[test]
    fn test_method_resolution_cache() {
        let bundle_name = BundleName::from("test");
        let mut trait_system = TraitSystem::new(bundle_name);
        
        let target_type = NovaType::Primitive(PrimitiveType::Integer);
        let method_name = "add";
        
        // First resolution
        let resolution1 = trait_system.resolve_method(&target_type, method_name, &[]);
        
        // Second resolution should be cached
        let _resolution2 = trait_system.resolve_method(&target_type, method_name, &[]);
        
        assert_eq!(trait_system.method_resolution_cache.len(), 1);
    }

    #[test]
    fn test_trait_system_statistics() {
        let bundle_name = BundleName::from("test");
        let trait_system = TraitSystem::new(bundle_name);
        
        let stats = trait_system.get_statistics();
        assert_eq!(stats.trait_count, 0);
        assert_eq!(stats.trait_implementation_count, 0);
        assert_eq!(stats.inherent_implementation_count, 0);
        assert_eq!(stats.coherence_violation_count, 0);
    }

    #[test]
    fn test_type_compatibility() {
        let bundle_name = BundleName::from("test");
        let trait_system = TraitSystem::new(bundle_name);
        
        let type1 = NovaType::Primitive(PrimitiveType::Integer);
        let type2 = NovaType::Primitive(PrimitiveType::Integer);
        let type3 = NovaType::Primitive(PrimitiveType::Boolean);
        
        assert!(trait_system.types_compatible(&type1, &type2));
        assert!(!trait_system.types_compatible(&type1, &type3));
    }
}