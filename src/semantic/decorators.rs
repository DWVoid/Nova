//! Nova Decorator System
//!
//! This module implements Nova's decorator system including decorator resolution,
//! argument type checking, compile-time expansion, and metadata generation.
//! It integrates with the type system and visibility system to provide
//! compile-time code transformation and validation.

use crate::syntax::ast::{Decorator, Exp, Definition, TopItem};
use crate::lexical::token::{Span, Position};
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName};
use super::bundle::BundleName;
use super::namespace::NamespacePath;
use super::types::{NovaType, TypeSystem};
use super::visibility::VisibilitySystem;
use std::collections::{HashMap, HashSet};

/// Core decorator system manager
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorSystem {
    /// Bundle being analyzed
    bundle_name: BundleName,
    /// Resolved decorator definitions
    decorator_definitions: HashMap<QualifiedName, DecoratorDefinition>,
    /// Applied decorators and their resolutions
    applied_decorators: HashMap<DecoratorApplicationId, ResolvedDecorator>,
    /// Decorator expansion results
    expansion_results: HashMap<DecoratorApplicationId, DecoratorExpansion>,
    /// Decorator composition chains
    composition_chains: HashMap<QualifiedName, Vec<DecoratorApplicationId>>,
}

/// Unique identifier for decorator applications
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct DecoratorApplicationId {
    /// Target definition being decorated
    target: QualifiedName,
    /// Decorator name
    decorator_name: String,
    /// Application order (for multiple decorators on same target)
    application_order: usize,
}

/// Decorator definition extracted from AST
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorDefinition {
    /// Qualified name of the decorator
    name: QualifiedName,
    /// Decorator function signature
    signature: DecoratorSignature,
    /// Target kinds this decorator can be applied to
    valid_targets: HashSet<DecoratorTargetKind>,
    /// Decorator implementation
    implementation: DecoratorImplementation,
    /// Source location for error reporting
    span: Span,
}

/// Decorator function signature
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorSignature {
    /// Decorator name
    name: String,
    /// Parameter types and names
    parameters: Vec<DecoratorParameter>,
    /// Return type (usually the transformed target)
    return_type: DecoratorReturnType,
    /// Compile-time constraints
    constraints: Vec<DecoratorConstraint>,
}

/// Decorator parameter specification
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorParameter {
    /// Parameter name
    name: String,
    /// Parameter type
    param_type: NovaType,
    /// Whether parameter is optional
    is_optional: bool,
    /// Default value if optional
    default_value: Option<DecoratorValue>,
}

/// Return type specification for decorators
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorReturnType {
    /// Returns the same type as the target
    SameAsTarget,
    /// Returns a specific type
    SpecificType(NovaType),
    /// Returns metadata only (no code transformation)
    MetadataOnly,
    /// Returns validation result
    ValidationResult,
}

/// Decorator constraint for validation
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorConstraint {
    /// Target must have specific type
    TargetTypeConstraint(NovaType),
    /// Target must be in specific namespace
    NamespaceConstraint(NamespacePath),
    /// Target must have specific visibility
    VisibilityConstraint(super::visibility::VisibilityRule),
    /// Custom validation constraint
    CustomConstraint(String, DecoratorValue),
}

/// Valid target kinds for decorators
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum DecoratorTargetKind {
    /// Can decorate type definitions
    TypeDefinition,
    /// Can decorate function definitions
    FunctionDefinition,
    /// Can decorate variable definitions
    VariableDefinition,
    /// Can decorate field definitions
    FieldDefinition,
    /// Can decorate trait definitions
    TraitDefinition,
    /// Can decorate implementation blocks
    ImplementationBlock,
    /// Can decorate entire bundles
    BundleDefinition,
    /// Can decorate namespace declarations
    NamespaceDefinition,
}

/// Decorator implementation
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorImplementation {
    /// Built-in decorator with native implementation
    BuiltIn(BuiltInDecorator),
    /// User-defined decorator with function implementation
    UserDefined(QualifiedName),
    /// External decorator from another bundle
    External(BundleName, QualifiedName),
}

/// Built-in decorator types
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum BuiltInDecorator {
    /// Deprecated decorator for deprecation warnings
    Deprecated,
    /// Test decorator for test functions
    Test,
    /// Inline decorator for function inlining
    Inline,
    /// Export decorator for symbol exports
    Export,
    /// Documentation decorator
    Doc,
    /// Conditional compilation decorator
    ConditionalCompilation,
    /// Performance profiling decorator
    Profile,
    /// Memory management decorator
    MemoryManaged,
}

/// Resolved decorator application
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ResolvedDecorator {
    /// Application identifier
    id: DecoratorApplicationId,
    /// Decorator definition
    definition: DecoratorDefinition,
    /// Resolved arguments
    arguments: Vec<ResolvedDecoratorArgument>,
    /// Target being decorated
    target: DecoratorTarget,
    /// Resolution status
    resolution_status: DecoratorResolutionStatus,
    /// Application context
    context: DecoratorContext,
}

/// Resolved decorator argument
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ResolvedDecoratorArgument {
    /// Parameter name
    parameter_name: String,
    /// Resolved value
    value: DecoratorValue,
    /// Type of the resolved value
    value_type: NovaType,
    /// Source location
    source_span: Span,
}

/// Decorator value types
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorValue {
    /// Literal integer value
    Integer(i64),
    /// Literal float value
    Float(f64),
    /// Literal string value
    String(String),
    /// Literal boolean value
    Boolean(bool),
    /// Array of values
    Array(Vec<DecoratorValue>),
    /// Object/record value
    Object(HashMap<String, DecoratorValue>),
    /// Type reference
    TypeReference(NovaType),
    /// Symbol reference
    SymbolReference(QualifiedName),
    /// Expression value (compile-time evaluated)
    Expression(Exp),
}

/// Target being decorated
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorTarget {
    /// Type definition target
    TypeDefinition {
        definition: Definition,
        qualified_name: QualifiedName,
    },
    /// Function definition target
    FunctionDefinition {
        definition: Definition,
        qualified_name: QualifiedName,
    },
    /// Variable definition target
    VariableDefinition {
        definition: Definition,
        qualified_name: QualifiedName,
    },
    /// Bundle-level target
    BundleTarget {
        bundle_name: BundleName,
    },
}

/// Status of decorator resolution
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum DecoratorResolutionStatus {
    /// Successfully resolved and validated
    Resolved,
    /// Failed to resolve decorator
    UnresolvedDecorator(String),
    /// Failed to resolve arguments
    UnresolvedArguments(Vec<String>),
    /// Type checking failed
    TypeCheckFailed(String),
    /// Target validation failed
    TargetValidationFailed(String),
    /// Constraint validation failed
    ConstraintValidationFailed(String),
}

/// Context for decorator application
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorContext {
    /// Current namespace
    namespace: NamespacePath,
    /// Visible symbols at application site
    visible_symbols: HashMap<String, QualifiedName>,
    /// Type environment at application site
    type_environment: HashMap<String, NovaType>,
    /// Application order in composition
    composition_order: usize,
}

/// Result of decorator expansion
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorExpansion {
    /// Application identifier
    application_id: DecoratorApplicationId,
    /// Type of expansion performed
    expansion_type: DecoratorExpansionType,
    /// Generated code or metadata
    result: DecoratorExpansionResult,
    /// Expansion diagnostics
    diagnostics: Vec<SemanticDiagnostic>,
}

/// Type of decorator expansion
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum DecoratorExpansionType {
    /// Code generation/transformation
    CodeGeneration,
    /// Metadata addition
    MetadataAddition,
    /// Compile-time validation
    CompileTimeValidation,
    /// Conditional compilation
    ConditionalCompilation,
    /// Performance instrumentation
    Instrumentation,
}

/// Result of decorator expansion
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DecoratorExpansionResult {
    /// Generated code statements
    GeneratedCode(Vec<GeneratedStatement>),
    /// Added metadata
    Metadata(DecoratorMetadata),
    /// Validation result
    ValidationResult(ValidationResult),
    /// Transformation result
    Transformation(TransformationResult),
    /// No expansion needed
    NoExpansion,
}

/// Generated statement from decorator expansion
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct GeneratedStatement {
    /// Generated AST statement
    statement: TopItem,
    /// Generation context
    context: String,
    /// Source attribution
    source_span: Span,
}

/// Metadata added by decorators
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorMetadata {
    /// Metadata key-value pairs
    attributes: HashMap<String, DecoratorValue>,
    /// Metadata category
    category: MetadataCategory,
    /// Retention policy
    retention: MetadataRetention,
}

/// Category of metadata
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum MetadataCategory {
    /// Documentation metadata
    Documentation,
    /// Debugging information
    Debug,
    /// Performance hints
    Performance,
    /// Deprecation information
    Deprecation,
    /// Test configuration
    Testing,
    /// Custom metadata
    Custom(String),
}

/// Metadata retention policy
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum MetadataRetention {
    /// Available at compile time only
    CompileTime,
    /// Available at runtime
    Runtime,
    /// Available in documentation
    Documentation,
}

/// Validation result from decorator
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ValidationResult {
    /// Whether validation passed
    is_valid: bool,
    /// Validation messages
    messages: Vec<String>,
    /// Suggestions for fixing issues
    suggestions: Vec<String>,
}

/// Transformation result from decorator
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TransformationResult {
    /// Transformed target
    transformed_target: DecoratorTarget,
    /// Additional generated definitions
    additional_definitions: Vec<Definition>,
    /// Transformation description
    transformation_description: String,
}

/// Statistics about the decorator system
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DecoratorStatistics {
    pub total_decorators: usize,
    pub builtin_decorators: usize,
    pub user_defined_decorators: usize,
    pub resolved_applications: usize,
    pub failed_applications: usize,
    pub generated_code_statements: usize,
    pub metadata_entries: usize,
}

impl DecoratorSystem {
    /// Create a new decorator system for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        Self {
            bundle_name,
            decorator_definitions: HashMap::new(),
            applied_decorators: HashMap::new(),
            expansion_results: HashMap::new(),
            composition_chains: HashMap::new(),
        }
    }

    /// Process decorators in the semantic analysis pipeline
    pub fn process_decorators(
        &mut self,
        definitions: &HashMap<QualifiedName, Definition>,
        type_system: &TypeSystem,
        visibility_system: &VisibilitySystem,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Phase 1: Discover and register decorator definitions
        self.discover_decorators(definitions, diagnostics);

        // Phase 2: Resolve decorator applications
        self.resolve_decorator_applications(definitions, type_system, visibility_system, diagnostics);

        // Phase 3: Validate decorator constraints
        self.validate_decorator_constraints(type_system, visibility_system, diagnostics);

        // Phase 4: Expand decorators and generate code/metadata
        self.expand_decorators(diagnostics);

        // Phase 5: Build composition chains for multiple decorators
        self.build_composition_chains(diagnostics);
    }

    /// Phase 1: Discover decorator definitions in the bundle
    fn discover_decorators(
        &mut self,
        definitions: &HashMap<QualifiedName, Definition>,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Register built-in decorators first
        self.register_builtin_decorators();

        // Discover user-defined decorators
        for (qualified_name, definition) in definitions {
            if self.is_decorator_definition(definition) {
                match self.extract_decorator_definition(qualified_name, definition) {
                    Ok(decorator_def) => {
                        self.decorator_definitions.insert(qualified_name.clone(), decorator_def);
                    }
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                    }
                }
            }
        }
    }

    /// Register built-in decorators
    fn register_builtin_decorators(&mut self) {
        let builtins = [
            (BuiltInDecorator::Deprecated, vec![DecoratorTargetKind::FunctionDefinition, DecoratorTargetKind::TypeDefinition]),
            (BuiltInDecorator::Test, vec![DecoratorTargetKind::FunctionDefinition]),
            (BuiltInDecorator::Inline, vec![DecoratorTargetKind::FunctionDefinition]),
            (BuiltInDecorator::Export, vec![DecoratorTargetKind::TypeDefinition, DecoratorTargetKind::FunctionDefinition]),
            (BuiltInDecorator::Doc, vec![DecoratorTargetKind::TypeDefinition, DecoratorTargetKind::FunctionDefinition, DecoratorTargetKind::FieldDefinition]),
            (BuiltInDecorator::ConditionalCompilation, vec![DecoratorTargetKind::FunctionDefinition, DecoratorTargetKind::TypeDefinition]),
            (BuiltInDecorator::Profile, vec![DecoratorTargetKind::FunctionDefinition]),
            (BuiltInDecorator::MemoryManaged, vec![DecoratorTargetKind::TypeDefinition]),
        ];

        for (builtin, targets) in builtins {
            let qualified_name = QualifiedName {
                bundle: self.bundle_name.clone(),
                namespace: vec!["builtin".to_string()],
                name: format!("{:?}", builtin).to_lowercase(),
            };

            let decorator_def = DecoratorDefinition {
                name: qualified_name.clone(),
                signature: self.create_builtin_signature(&builtin),
                valid_targets: targets.into_iter().collect(),
                implementation: DecoratorImplementation::BuiltIn(builtin),
                span: Span::single(Position::start()),
            };

            self.decorator_definitions.insert(qualified_name, decorator_def);
        }
    }

    /// Create signature for built-in decorator
    fn create_builtin_signature(&self, builtin: &BuiltInDecorator) -> DecoratorSignature {
        let (name, parameters, return_type) = match builtin {
            BuiltInDecorator::Deprecated => (
                "deprecated".to_string(),
                vec![
                    DecoratorParameter {
                        name: "message".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: true,
                        default_value: Some(DecoratorValue::String("This item is deprecated".to_string())),
                    },
                    DecoratorParameter {
                        name: "since".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: true,
                        default_value: None,
                    },
                ],
                DecoratorReturnType::MetadataOnly,
            ),
            BuiltInDecorator::Test => (
                "test".to_string(),
                vec![
                    DecoratorParameter {
                        name: "name".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: true,
                        default_value: None,
                    },
                ],
                DecoratorReturnType::MetadataOnly,
            ),
            BuiltInDecorator::Inline => (
                "inline".to_string(),
                vec![
                    DecoratorParameter {
                        name: "always".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::Boolean),
                        is_optional: true,
                        default_value: Some(DecoratorValue::Boolean(false)),
                    },
                ],
                DecoratorReturnType::SameAsTarget,
            ),
            BuiltInDecorator::Export => (
                "export".to_string(),
                vec![
                    DecoratorParameter {
                        name: "name".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: true,
                        default_value: None,
                    },
                ],
                DecoratorReturnType::SameAsTarget,
            ),
            BuiltInDecorator::Doc => (
                "doc".to_string(),
                vec![
                    DecoratorParameter {
                        name: "content".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: false,
                        default_value: None,
                    },
                ],
                DecoratorReturnType::MetadataOnly,
            ),
            BuiltInDecorator::ConditionalCompilation => (
                "cfg".to_string(),
                vec![
                    DecoratorParameter {
                        name: "condition".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: false,
                        default_value: None,
                    },
                ],
                DecoratorReturnType::SameAsTarget,
            ),
            BuiltInDecorator::Profile => (
                "profile".to_string(),
                vec![],
                DecoratorReturnType::SameAsTarget,
            ),
            BuiltInDecorator::MemoryManaged => (
                "memory_managed".to_string(),
                vec![
                    DecoratorParameter {
                        name: "strategy".to_string(),
                        param_type: NovaType::Primitive(super::types::PrimitiveType::String),
                        is_optional: true,
                        default_value: Some(DecoratorValue::String("automatic".to_string())),
                    },
                ],
                DecoratorReturnType::SameAsTarget,
            ),
        };

        DecoratorSignature {
            name,
            parameters,
            return_type,
            constraints: Vec::new(),
        }
    }

    /// Check if definition is a decorator definition
    fn is_decorator_definition(&self, _definition: &Definition) -> bool {
        // For now, we'll focus on built-in decorators
        // In a full implementation, this would check for decorator attributes
        // or specific naming conventions that identify decorator functions
        false
    }

    /// Extract decorator definition from AST
    fn extract_decorator_definition(
        &self,
        qualified_name: &QualifiedName,
        _definition: &Definition,
    ) -> Result<DecoratorDefinition, SemanticDiagnostic> {
        // Placeholder for user-defined decorator extraction
        Err(SemanticDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: format!("User-defined decorators not yet implemented: {}", qualified_name.name),
            location: Position::start(),
            category: DiagnosticCategory::DecoratorError,
        })
    }

    /// Phase 2: Resolve decorator applications
    fn resolve_decorator_applications(
        &mut self,
        definitions: &HashMap<QualifiedName, Definition>,
        type_system: &TypeSystem,
        _visibility_system: &VisibilitySystem,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (qualified_name, definition) in definitions {
            if !definition.decorators.is_empty() {
                for (index, decorator) in definition.decorators.iter().enumerate() {
                    let application_id = DecoratorApplicationId {
                        target: qualified_name.clone(),
                        decorator_name: decorator.name.value.clone(),
                        application_order: index,
                    };

                    match self.resolve_decorator_application(
                        &application_id,
                        decorator,
                        definition,
                        qualified_name,
                        type_system,
                    ) {
                        Ok(resolved) => {
                            self.applied_decorators.insert(application_id, resolved);
                        }
                        Err(diagnostic) => {
                            diagnostics.push(diagnostic);
                        }
                    }
                }
            }
        }
    }

    /// Resolve a single decorator application
    fn resolve_decorator_application(
        &self,
        application_id: &DecoratorApplicationId,
        decorator: &Decorator,
        definition: &Definition,
        qualified_name: &QualifiedName,
        type_system: &TypeSystem,
    ) -> Result<ResolvedDecorator, SemanticDiagnostic> {
        // Find decorator definition
        let decorator_name = &decorator.name.value;
        let decorator_qualified_name = QualifiedName {
            bundle: self.bundle_name.clone(),
            namespace: vec!["builtin".to_string()],
            name: decorator_name.clone(),
        };

        let decorator_definition = self.decorator_definitions.get(&decorator_qualified_name)
            .ok_or_else(|| SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Unknown decorator: @{}", decorator_name),
                location: decorator.span.start,
                category: DiagnosticCategory::DecoratorError,
            })?.clone();

        // Validate target kind
        let target_kind = self.get_target_kind(definition);
        if !decorator_definition.valid_targets.contains(&target_kind) {
            return Err(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "Decorator @{} cannot be applied to {:?}",
                    decorator_name, target_kind
                ),
                location: decorator.span.start,
                category: DiagnosticCategory::DecoratorError,
            });
        }

        // Resolve arguments
        let args = decorator.args.as_ref().map(|args| args.as_slice()).unwrap_or(&[]);
        let resolved_arguments = self.resolve_decorator_arguments(
            args,
            &decorator_definition.signature,
            type_system,
        )?;

        // Create target
        let target = self.create_decorator_target(definition, qualified_name);

        // Create context
        let context = DecoratorContext {
            namespace: super::namespace::NamespacePath(qualified_name.namespace.clone()),
            visible_symbols: HashMap::new(), // TODO: Populate from scope
            type_environment: HashMap::new(), // TODO: Populate from type system
            composition_order: application_id.application_order,
        };

        Ok(ResolvedDecorator {
            id: application_id.clone(),
            definition: decorator_definition,
            arguments: resolved_arguments,
            target,
            resolution_status: DecoratorResolutionStatus::Resolved,
            context,
        })
    }

    /// Get target kind from definition
    fn get_target_kind(&self, definition: &Definition) -> DecoratorTargetKind {
        match &definition.expr {
            crate::syntax::ast::DefExpr::Exp(_) => DecoratorTargetKind::FunctionDefinition,
            crate::syntax::ast::DefExpr::Struct(_) => DecoratorTargetKind::TypeDefinition,
            crate::syntax::ast::DefExpr::Enum(_) => DecoratorTargetKind::TypeDefinition,
            crate::syntax::ast::DefExpr::Variant(_) => DecoratorTargetKind::TypeDefinition,
            crate::syntax::ast::DefExpr::Trait(_) => DecoratorTargetKind::TraitDefinition,
        }
    }

    /// Create decorator target from definition
    fn create_decorator_target(&self, definition: &Definition, qualified_name: &QualifiedName) -> DecoratorTarget {
        match self.get_target_kind(definition) {
            DecoratorTargetKind::FunctionDefinition => DecoratorTarget::FunctionDefinition {
                definition: definition.clone(),
                qualified_name: qualified_name.clone(),
            },
            DecoratorTargetKind::TypeDefinition | DecoratorTargetKind::TraitDefinition => DecoratorTarget::TypeDefinition {
                definition: definition.clone(),
                qualified_name: qualified_name.clone(),
            },
            DecoratorTargetKind::VariableDefinition => DecoratorTarget::VariableDefinition {
                definition: definition.clone(),
                qualified_name: qualified_name.clone(),
            },
            _ => DecoratorTarget::TypeDefinition {
                definition: definition.clone(),
                qualified_name: qualified_name.clone(),
            },
        }
    }

    /// Resolve decorator arguments
    fn resolve_decorator_arguments(
        &self,
        arguments: &[Exp],
        signature: &DecoratorSignature,
        _type_system: &TypeSystem,
    ) -> Result<Vec<ResolvedDecoratorArgument>, SemanticDiagnostic> {
        let mut resolved_args = Vec::new();

        // For now, we'll do simple argument resolution
        // In a full implementation, this would type-check arguments against signature
        for (index, arg) in arguments.iter().enumerate() {
            if index < signature.parameters.len() {
                let param = &signature.parameters[index];
                let resolved_arg = ResolvedDecoratorArgument {
                    parameter_name: param.name.clone(),
                    value: self.convert_ast_to_decorator_value(arg),
                    value_type: param.param_type.clone(),
                    source_span: arg.span,
                };
                resolved_args.push(resolved_arg);
            }
        }

        // Add default values for missing optional parameters
        for param in &signature.parameters[arguments.len()..] {
            if param.is_optional {
                if let Some(default_value) = &param.default_value {
                    let resolved_arg = ResolvedDecoratorArgument {
                        parameter_name: param.name.clone(),
                        value: default_value.clone(),
                        value_type: param.param_type.clone(),
                        source_span: Span::single(Position::start()),
                    };
                    resolved_args.push(resolved_arg);
                }
            } else {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Missing required argument: {}", param.name),
                    location: Position::start(),
                    category: DiagnosticCategory::DecoratorError,
                });
            }
        }

        Ok(resolved_args)
    }

    /// Convert AST expression to decorator value
    fn convert_ast_to_decorator_value(&self, exp: &Exp) -> DecoratorValue {
        match &exp.kind {
            crate::syntax::ast::ExpKind::Number(n) => {
                // Try to parse as integer first, then float
                if let Ok(i) = n.parse::<i64>() {
                    DecoratorValue::Integer(i)
                } else if let Ok(f) = n.parse::<f64>() {
                    DecoratorValue::Float(f)
                } else {
                    DecoratorValue::Expression(exp.clone())
                }
            }
            crate::syntax::ast::ExpKind::String(s) => DecoratorValue::String(s.clone()),
            crate::syntax::ast::ExpKind::Bool(b) => DecoratorValue::Boolean(*b),
            _ => DecoratorValue::Expression(exp.clone()),
        }
    }

    /// Phase 3: Validate decorator constraints
    fn validate_decorator_constraints(
        &mut self,
        _type_system: &TypeSystem,
        _visibility_system: &VisibilitySystem,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement constraint validation
        // This would validate type constraints, visibility constraints, etc.
    }

    /// Phase 4: Expand decorators and generate code/metadata
    fn expand_decorators(&mut self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        for (application_id, resolved_decorator) in &self.applied_decorators.clone() {
            match self.expand_decorator(resolved_decorator) {
                Ok(expansion) => {
                    self.expansion_results.insert(application_id.clone(), expansion);
                }
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    /// Expand a single decorator
    fn expand_decorator(&self, resolved_decorator: &ResolvedDecorator) -> Result<DecoratorExpansion, SemanticDiagnostic> {
        let expansion_result = match &resolved_decorator.definition.implementation {
            DecoratorImplementation::BuiltIn(builtin) => {
                self.expand_builtin_decorator(builtin, resolved_decorator)?
            }
            DecoratorImplementation::UserDefined(_) => {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: "User-defined decorators not yet implemented".to_string(),
                    location: Position::start(),
                    category: DiagnosticCategory::DecoratorError,
                });
            }
            DecoratorImplementation::External(_, _) => {
                return Err(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: "External decorators not yet implemented".to_string(),
                    location: Position::start(),
                    category: DiagnosticCategory::DecoratorError,
                });
            }
        };

        Ok(DecoratorExpansion {
            application_id: resolved_decorator.id.clone(),
            expansion_type: self.get_expansion_type(&resolved_decorator.definition.implementation),
            result: expansion_result,
            diagnostics: Vec::new(),
        })
    }

    /// Get expansion type for implementation
    fn get_expansion_type(&self, implementation: &DecoratorImplementation) -> DecoratorExpansionType {
        match implementation {
            DecoratorImplementation::BuiltIn(builtin) => match builtin {
                BuiltInDecorator::Deprecated => DecoratorExpansionType::MetadataAddition,
                BuiltInDecorator::Test => DecoratorExpansionType::MetadataAddition,
                BuiltInDecorator::Inline => DecoratorExpansionType::CodeGeneration,
                BuiltInDecorator::Export => DecoratorExpansionType::MetadataAddition,
                BuiltInDecorator::Doc => DecoratorExpansionType::MetadataAddition,
                BuiltInDecorator::ConditionalCompilation => DecoratorExpansionType::ConditionalCompilation,
                BuiltInDecorator::Profile => DecoratorExpansionType::Instrumentation,
                BuiltInDecorator::MemoryManaged => DecoratorExpansionType::CodeGeneration,
            },
            _ => DecoratorExpansionType::MetadataAddition,
        }
    }

    /// Expand built-in decorator
    fn expand_builtin_decorator(
        &self,
        builtin: &BuiltInDecorator,
        resolved_decorator: &ResolvedDecorator,
    ) -> Result<DecoratorExpansionResult, SemanticDiagnostic> {
        match builtin {
            BuiltInDecorator::Deprecated => {
                let message = resolved_decorator.arguments.iter()
                    .find(|arg| arg.parameter_name == "message")
                    .map(|arg| match &arg.value {
                        DecoratorValue::String(s) => s.clone(),
                        _ => "This item is deprecated".to_string(),
                    })
                    .unwrap_or_else(|| "This item is deprecated".to_string());

                let mut attributes = HashMap::new();
                attributes.insert("message".to_string(), DecoratorValue::String(message));

                let metadata = DecoratorMetadata {
                    attributes,
                    category: MetadataCategory::Deprecation,
                    retention: MetadataRetention::CompileTime,
                };

                Ok(DecoratorExpansionResult::Metadata(metadata))
            }
            BuiltInDecorator::Test => {
                let mut attributes = HashMap::new();
                attributes.insert("is_test".to_string(), DecoratorValue::Boolean(true));

                let metadata = DecoratorMetadata {
                    attributes,
                    category: MetadataCategory::Testing,
                    retention: MetadataRetention::Runtime,
                };

                Ok(DecoratorExpansionResult::Metadata(metadata))
            }
            BuiltInDecorator::Doc => {
                let content = resolved_decorator.arguments.iter()
                    .find(|arg| arg.parameter_name == "content")
                    .map(|arg| match &arg.value {
                        DecoratorValue::String(s) => s.clone(),
                        _ => "".to_string(),
                    })
                    .unwrap_or_default();

                let mut attributes = HashMap::new();
                attributes.insert("content".to_string(), DecoratorValue::String(content));

                let metadata = DecoratorMetadata {
                    attributes,
                    category: MetadataCategory::Documentation,
                    retention: MetadataRetention::Documentation,
                };

                Ok(DecoratorExpansionResult::Metadata(metadata))
            }
            _ => {
                // For other decorators, return no expansion for now
                Ok(DecoratorExpansionResult::NoExpansion)
            }
        }
    }

    /// Phase 5: Build composition chains
    fn build_composition_chains(&mut self, _diagnostics: &mut Vec<SemanticDiagnostic>) {
        let mut chains: HashMap<QualifiedName, Vec<DecoratorApplicationId>> = HashMap::new();

        for application_id in self.applied_decorators.keys() {
            chains.entry(application_id.target.clone())
                .or_insert_with(Vec::new)
                .push(application_id.clone());
        }

        // Sort by application order
        for chain in chains.values_mut() {
            chain.sort_by_key(|id| id.application_order);
        }

        self.composition_chains = chains;
    }

    /// Get statistics about the decorator system
    #[allow(dead_code)]
    pub fn get_statistics(&self) -> DecoratorStatistics {
        let builtin_count = self.decorator_definitions.values()
            .filter(|def| matches!(def.implementation, DecoratorImplementation::BuiltIn(_)))
            .count();

        let user_defined_count = self.decorator_definitions.len() - builtin_count;

        let resolved_count = self.applied_decorators.values()
            .filter(|resolved| matches!(resolved.resolution_status, DecoratorResolutionStatus::Resolved))
            .count();

        let failed_count = self.applied_decorators.len() - resolved_count;

        let generated_code_count = self.expansion_results.values()
            .filter_map(|expansion| match &expansion.result {
                DecoratorExpansionResult::GeneratedCode(statements) => Some(statements.len()),
                _ => None,
            })
            .sum();

        let metadata_count = self.expansion_results.values()
            .filter(|expansion| matches!(expansion.result, DecoratorExpansionResult::Metadata(_)))
            .count();

        DecoratorStatistics {
            total_decorators: self.decorator_definitions.len(),
            builtin_decorators: builtin_count,
            user_defined_decorators: user_defined_count,
            resolved_applications: resolved_count,
            failed_applications: failed_count,
            generated_code_statements: generated_code_count,
            metadata_entries: metadata_count,
        }
    }
}

impl Default for DecoratorSystem {
    fn default() -> Self {
        Self::new(BundleName::from("default"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::{Name, ExpKind};

    #[test]
    fn test_decorator_system_creation() {
        let bundle_name = BundleName::from("test");
        let decorator_system = DecoratorSystem::new(bundle_name.clone());
        
        assert_eq!(decorator_system.bundle_name, bundle_name);
        assert!(decorator_system.decorator_definitions.is_empty());
    }

    #[test]
    fn test_builtin_decorator_registration() {
        let mut decorator_system = DecoratorSystem::new(BundleName::from("test"));
        decorator_system.register_builtin_decorators();
        
        assert!(!decorator_system.decorator_definitions.is_empty());
        
        // Check for specific built-in decorators
        let deprecated_name = QualifiedName {
            bundle: BundleName::from("test"),
            namespace: vec!["builtin".to_string()],
            name: "deprecated".to_string(),
        };
        
        assert!(decorator_system.decorator_definitions.contains_key(&deprecated_name));
    }

    #[test]
    fn test_decorator_signature_creation() {
        let decorator_system = DecoratorSystem::new(BundleName::from("test"));
        let signature = decorator_system.create_builtin_signature(&BuiltInDecorator::Deprecated);
        
        assert_eq!(signature.name, "deprecated");
        assert_eq!(signature.parameters.len(), 2);
        assert!(signature.parameters[0].is_optional);
        assert!(signature.parameters[1].is_optional);
    }

    #[test]
    fn test_ast_to_decorator_value_conversion() {
        let decorator_system = DecoratorSystem::new(BundleName::from("test"));
        
        let string_exp = Exp {
            kind: ExpKind::String("test".to_string()),
            span: Span::single(Position::start()),
        };
        
        let value = decorator_system.convert_ast_to_decorator_value(&string_exp);
        match value {
            DecoratorValue::String(s) => assert_eq!(s, "test"),
            _ => panic!("Expected string value"),
        }
    }

    #[test]
    fn test_target_kind_detection() {
        let decorator_system = DecoratorSystem::new(BundleName::from("test"));
        
        let function_def = Definition {
            decorators: Vec::new(),
            visibility: None,
            name: Name {
                value: "test_func".to_string(),
                span: Span::single(Position::start()),
            },
            type_spec: None,
            expr: crate::syntax::ast::DefExpr::Exp(Exp {
                kind: ExpKind::Nil,
                span: Span::single(Position::start()),
            }),
            span: Span::single(Position::start()),
        };
        
        let target_kind = decorator_system.get_target_kind(&function_def);
        assert_eq!(target_kind, DecoratorTargetKind::FunctionDefinition);
    }

    #[test]
    fn test_decorator_statistics() {
        let mut decorator_system = DecoratorSystem::new(BundleName::from("test"));
        decorator_system.register_builtin_decorators();
        
        let stats = decorator_system.get_statistics();
        assert!(stats.total_decorators > 0);
        assert!(stats.builtin_decorators > 0);
        assert_eq!(stats.user_defined_decorators, 0);
    }

    #[test]
    fn test_expansion_type_detection() {
        let decorator_system = DecoratorSystem::new(BundleName::from("test"));
        
        let builtin_impl = DecoratorImplementation::BuiltIn(BuiltInDecorator::Deprecated);
        let expansion_type = decorator_system.get_expansion_type(&builtin_impl);
        
        assert_eq!(expansion_type, DecoratorExpansionType::MetadataAddition);
    }
}