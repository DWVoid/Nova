//! Nova Type System Implementation
//!
//! This module implements the core type system for Nova including type definitions,
//! type checking, type resolution, and constraint solving. It builds upon the symbol
//! table system from Step 3 to provide comprehensive type analysis.

use crate::syntax::ast::{TopItem, Definition, DefExpr, Exp, TypeName};
use crate::lexical::{Position, Span};
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName};
use super::bundle::BundleName;
use super::namespace::{NamespaceTree, NamespacePath};
use super::symbols::SymbolTableBuilder;
use std::collections::{HashMap, HashSet};

/// Core type system manager
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeSystem {
    /// Current bundle being analyzed
    bundle_name: BundleName,
    /// Type environment containing all type definitions
    type_environment: TypeEnvironment,
    /// Type constraints collected during analysis
    active_constraints: Vec<TypeConstraint>,
    /// Type variable generator
    type_var_counter: usize,
    /// Type checking context stack
    context_stack: Vec<TypeContext>,
}

/// Type environment containing all type information
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct TypeEnvironment {
    /// Type definitions from all bundles
    pub bundle_types: HashMap<QualifiedName, TypeDefinition>,
    /// Imported type references
    pub imported_types: HashMap<String, TypeReference>,
    /// Type aliases resolved to their target types
    pub type_aliases: HashMap<String, NovaType>,
    /// Primitive type definitions
    pub primitive_types: HashMap<String, PrimitiveTypeInfo>,
    /// Generic type parameter bindings
    pub type_parameter_bindings: HashMap<String, NovaType>,
}

/// A type in the Nova type system
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum NovaType {
    /// Primitive types (integer, boolean, etc.)
    Primitive(PrimitiveType),
    /// Named type with optional type arguments
    Named(QualifiedName, Vec<NovaType>),
    /// Function type
    Function(Vec<NovaType>, Box<NovaType>),
    /// Trait object type
    Trait(QualifiedName, Vec<NovaType>),
    /// Type variable for inference
    Variable(TypeVariable),
    /// Unit type (represents no value)
    Unit,
    /// Error type (for error recovery)
    Error,
}

/// Primitive type kinds
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PrimitiveType {
    Integer,
    Float,
    Boolean,
    String,
    Unit,
}

/// Type variable for type inference
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct TypeVariable {
    pub id: usize,
    pub name: Option<String>,
}

/// Type definition in the semantic model
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TypeDefinition {
    /// Struct type with fields
    Struct {
        fields: HashMap<String, (NovaType, FieldMetadata)>,
        type_parameters: Vec<TypeParameter>,
        visibility: super::Visibility,
    },
    /// Enum type with variants
    Enum {
        base_type: NovaType,
        variants: HashMap<String, i64>,
        type_parameters: Vec<TypeParameter>,
        visibility: super::Visibility,
    },
    /// Variant type with cases
    Variant {
        cases: HashMap<String, NovaType>,
        type_parameters: Vec<TypeParameter>,
        visibility: super::Visibility,
    },
    /// Trait type with method signatures
    Trait {
        signatures: HashMap<String, FunctionSignature>,
        type_parameters: Vec<TypeParameter>,
        visibility: super::Visibility,
    },
    /// Type alias
    Alias {
        target: NovaType,
        type_parameters: Vec<TypeParameter>,
        visibility: super::Visibility,
    },
}

/// Type reference to another type
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeReference {
    /// Qualified name of the referenced type
    pub qualified_name: QualifiedName,
    /// Type arguments if generic
    pub type_arguments: Vec<NovaType>,
}

/// Type parameter for generic types
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeParameter {
    /// Parameter name
    pub name: String,
    /// Trait bounds
    pub bounds: Vec<NovaType>,
    /// Variance (covariant, contravariant, invariant)
    pub variance: Variance,
}

/// Variance of type parameters
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Variance {
    Covariant,
    Contravariant,
    Invariant,
}

/// Field metadata for struct fields
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FieldMetadata {
    /// Field visibility
    pub visibility: super::Visibility,
    /// Whether field is mutable
    pub is_mutable: bool,
    /// Field decorators
    pub decorators: Vec<String>, // Simplified for now
}

/// Function signature with type information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FunctionSignature {
    /// Function name
    pub name: String,
    /// Type parameters
    pub type_parameters: Vec<TypeParameter>,
    /// Parameters with types
    pub parameters: Vec<Parameter>,
    /// Return type
    pub return_type: NovaType,
    /// Whether function is const
    pub is_const: bool,
    /// Function constraints
    pub constraints: Vec<TypeConstraint>,
}

/// Function parameter with type information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub param_type: NovaType,
    /// Whether parameter is mutable
    pub is_mutable: bool,
}

/// Type constraint for constraint solving
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum TypeConstraint {
    /// Two types must be equal
    Equality(NovaType, NovaType),
    /// Type must implement trait
    TraitBound(NovaType, QualifiedName),
    /// Type must be a subtype of another
    Subtype(NovaType, NovaType),
    /// Type must have a specific field
    HasField(NovaType, String, NovaType),
    /// Type must be callable with specific signature
    Callable(NovaType, Vec<NovaType>, NovaType),
}

/// Type checking context
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeContext {
    /// Current namespace being analyzed
    pub current_namespace: NamespacePath,
    /// Expected return type for current function
    pub expected_return_type: Option<NovaType>,
    /// Local variable types
    pub local_variables: HashMap<String, NovaType>,
    /// Type parameter bindings in current scope
    pub type_parameters: HashMap<String, TypeParameter>,
    /// Whether we're in a const context
    pub is_const_context: bool,
}

/// Information about primitive types
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct PrimitiveTypeInfo {
    /// Type name
    pub name: String,
    /// Size in bytes (if known)
    pub size: Option<usize>,
    /// Supported operations
    pub operations: HashSet<String>,
}

/// Result of type checking
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TypeCheckResult {
    /// Type checking succeeded with inferred type
    Success(NovaType),
    /// Type checking failed with error
    Error(TypeCheckError),
    /// Type checking needs more information
    NeedsInference(Vec<TypeConstraint>),
}

/// Type checking error information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeCheckError {
    /// Error message
    pub message: String,
    /// Expected type (if applicable)
    pub expected: Option<NovaType>,
    /// Actual type found
    pub actual: Option<NovaType>,
    /// Source location
    pub location: Span,
    /// Suggestions for fixing the error
    pub suggestions: Vec<String>,
}

impl TypeSystem {
    /// Create a new type system for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        let mut type_system = Self {
            bundle_name,
            type_environment: TypeEnvironment::default(),
            active_constraints: Vec::new(),
            type_var_counter: 0,
            context_stack: Vec::new(),
        };

        // Initialize primitive types
        type_system.initialize_primitive_types();
        type_system
    }

    /// Initialize primitive type definitions
    fn initialize_primitive_types(&mut self) {
        let primitives = [
            ("integer", PrimitiveType::Integer, Some(8), vec!["add", "sub", "mul", "div", "mod", "eq", "ne", "lt", "le", "gt", "ge"]),
            ("float", PrimitiveType::Float, Some(8), vec!["add", "sub", "mul", "div", "eq", "ne", "lt", "le", "gt", "ge"]),
            ("boolean", PrimitiveType::Boolean, Some(1), vec!["and", "or", "not", "eq", "ne"]),
            ("string", PrimitiveType::String, None, vec!["add", "eq", "ne", "len"]),
            ("unit", PrimitiveType::Unit, Some(0), vec!["eq", "ne"]),
        ];

        for (name, prim_type, size, ops) in primitives {
            let operations = ops.into_iter().map(String::from).collect();
            
            self.type_environment.primitive_types.insert(
                name.to_string(),
                PrimitiveTypeInfo {
                    name: name.to_string(),
                    size,
                    operations,
                }
            );

            // Also add to type aliases for easy lookup
            self.type_environment.type_aliases.insert(
                name.to_string(),
                NovaType::Primitive(prim_type)
            );
        }
    }

    /// Build type environment from namespace tree and symbol tables
    pub fn build_from_namespace_tree(
        &mut self,
        namespace_tree: &NamespaceTree,
        symbol_table: &SymbolTableBuilder,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Phase 1: Collect all type definitions
        self.collect_type_definitions(namespace_tree, diagnostics);

        // Phase 2: Resolve type references and aliases
        self.resolve_type_references(namespace_tree, diagnostics);

        // Phase 3: Validate type definitions
        self.validate_type_definitions(diagnostics);

        // Phase 4: Build type constraint system
        self.build_constraint_system(namespace_tree, symbol_table, diagnostics);
    }

    /// Collect type definitions from namespace tree
    fn collect_type_definitions(
        &mut self,
        namespace_tree: &NamespaceTree,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (namespace_path, namespace_scope) in &namespace_tree.namespaces {
            for (symbol_name, local_def) in &namespace_scope.definitions {
                if let TopItem::Definition(def) = &local_def.item {
                    if let Some(type_def) = self.extract_type_definition(def, namespace_path) {
                        let qualified_name = QualifiedName {
                            bundle: namespace_tree.root.clone(),
                            namespace: namespace_path.0.clone(),
                            name: symbol_name.clone(),
                        };

                        // Check for type conflicts
                        if self.type_environment.bundle_types.contains_key(&qualified_name) {
                            diagnostics.push(SemanticDiagnostic {
                                severity: DiagnosticSeverity::Error,
                                message: format!(
                                    "Duplicate type definition: '{}' in namespace '{}'",
                                    symbol_name, namespace_path
                                ),
                                location: def.span.start,
                                category: DiagnosticCategory::TypeError,
                            });
                        } else {
                            self.type_environment.bundle_types.insert(qualified_name, type_def);
                        }
                    }
                }
            }
        }
    }

    /// Extract type definition from AST definition
    fn extract_type_definition(
        &self,
        def: &Definition,
        _namespace_path: &NamespacePath,
    ) -> Option<TypeDefinition> {
        let visibility = if def.visibility.is_some() {
            super::Visibility::Public
        } else {
            super::Visibility::Private
        };

        match &def.expr {
            DefExpr::Struct(struct_def) => {
                let mut fields = HashMap::new();
                
                for field in &struct_def.fields {
                    let field_type = self.convert_ast_type_to_nova_type(&field.type_spec.ty);
                    let field_metadata = FieldMetadata {
                        visibility: super::Visibility::Public, // TODO: Extract from field visibility
                        is_mutable: true, // TODO: Extract from field mutability
                        decorators: Vec::new(),
                    };
                    fields.insert(field.name.value.clone(), (field_type, field_metadata));
                }

                Some(TypeDefinition::Struct {
                    fields,
                    type_parameters: Vec::new(), // TODO: Extract type parameters
                    visibility,
                })
            }
            DefExpr::Enum(enum_def) => {
                let base_type = self.convert_ast_type_to_nova_type(&enum_def.type_spec.ty);

                let mut variants = HashMap::new();
                for (i, member) in enum_def.members.iter().enumerate() {
                    variants.insert(member.name.value.clone(), i as i64);
                }

                Some(TypeDefinition::Enum {
                    base_type,
                    variants,
                    type_parameters: Vec::new(),
                    visibility,
                })
            }
            DefExpr::Variant(variant_def) => {
                let mut cases = HashMap::new();
                for member in &variant_def.members {
                    let case_type = self.convert_ast_type_to_nova_type(&member.type_spec.ty);
                    cases.insert(member.name.value.clone(), case_type);
                }

                Some(TypeDefinition::Variant {
                    cases,
                    type_parameters: Vec::new(),
                    visibility,
                })
            }
            DefExpr::Trait(_trait_def) => {
                // TODO: Implement trait definition extraction
                Some(TypeDefinition::Trait {
                    signatures: HashMap::new(),
                    type_parameters: Vec::new(),
                    visibility,
                })
            }
            DefExpr::Exp(_exp) => {
                // Expression definitions don't create types directly
                None
            }
        }
    }

    /// Convert AST type name to Nova type
    fn convert_ast_type_to_nova_type(&self, type_name: &TypeName) -> NovaType {
        if type_name.parts.len() == 1 {
            let type_str = &type_name.parts[0].value;
            
            // Check if it's a primitive type
            if let Some(_) = self.type_environment.primitive_types.get(type_str) {
                return match type_str.as_str() {
                    "integer" => NovaType::Primitive(PrimitiveType::Integer),
                    "float" => NovaType::Primitive(PrimitiveType::Float),
                    "boolean" => NovaType::Primitive(PrimitiveType::Boolean),
                    "string" => NovaType::Primitive(PrimitiveType::String),
                    "unit" => NovaType::Primitive(PrimitiveType::Unit),
                    _ => NovaType::Error,
                };
            }
            
            // Check if it's a type alias
            if let Some(resolved_type) = self.type_environment.type_aliases.get(type_str) {
                return resolved_type.clone();
            }
        }

        // Handle qualified type names
        let qualified_name = QualifiedName {
            bundle: self.bundle_name.clone(),
            namespace: type_name.parts.iter().take(type_name.parts.len().saturating_sub(1))
                .map(|n| n.value.clone()).collect(),
            name: type_name.parts.last().unwrap().value.clone(),
        };

        NovaType::Named(qualified_name, Vec::new())
    }

    /// Resolve type references and aliases
    fn resolve_type_references(
        &mut self,
        _namespace_tree: &NamespaceTree,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement type reference resolution
        // This would resolve Named types to their actual definitions
        // and expand type aliases
    }

    /// Validate type definitions for consistency
    fn validate_type_definitions(&self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        for (qualified_name, type_def) in &self.type_environment.bundle_types {
            match type_def {
                TypeDefinition::Struct { fields, .. } => {
                    self.validate_struct_definition(qualified_name, fields, diagnostics);
                }
                TypeDefinition::Enum { variants, base_type, .. } => {
                    self.validate_enum_definition(qualified_name, variants, base_type, diagnostics);
                }
                TypeDefinition::Variant { cases, .. } => {
                    self.validate_variant_definition(qualified_name, cases, diagnostics);
                }
                _ => {
                    // Other type definitions validation
                }
            }
        }
    }

    /// Validate struct definition
    fn validate_struct_definition(
        &self,
        qualified_name: &QualifiedName,
        fields: &HashMap<String, (NovaType, FieldMetadata)>,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Check for empty structs
        if fields.is_empty() {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("Struct '{}' has no fields", qualified_name.name),
                location: Position::new_start(),
                category: DiagnosticCategory::TypeError,
            });
        }

        // Check for duplicate field names (should not happen, but defensive)
        let mut field_names = HashSet::new();
        for field_name in fields.keys() {
            if !field_names.insert(field_name) {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Duplicate field '{}' in struct '{}'", field_name, qualified_name.name),
                    location: Position::new_start(),
                    category: DiagnosticCategory::TypeError,
                });
            }
        }

        // TODO: Check for recursive type definitions
        // TODO: Validate field types exist and are accessible
    }

    /// Validate enum definition
    fn validate_enum_definition(
        &self,
        qualified_name: &QualifiedName,
        variants: &HashMap<String, i64>,
        base_type: &NovaType,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Check for empty enums
        if variants.is_empty() {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("Enum '{}' has no variants", qualified_name.name),
                location: Position::new_start(),
                category: DiagnosticCategory::TypeError,
            });
        }

        // Validate base type is appropriate for enum
        match base_type {
            NovaType::Primitive(PrimitiveType::Integer) => {
                // Good
            }
            _ => {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Enum '{}' base type must be integer", qualified_name.name),
                    location: Position::new_start(),
                    category: DiagnosticCategory::TypeError,
                });
            }
        }

        // Check for duplicate variant values
        let mut used_values = HashSet::new();
        for (variant_name, value) in variants {
            if !used_values.insert(value) {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    message: format!("Enum '{}' has duplicate value {} for variant '{}'", 
                        qualified_name.name, value, variant_name),
                    location: Position::new_start(),
                    category: DiagnosticCategory::TypeError,
                });
            }
        }
    }

    /// Validate variant definition
    fn validate_variant_definition(
        &self,
        qualified_name: &QualifiedName,
        cases: &HashMap<String, NovaType>,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Check for empty variants
        if cases.is_empty() {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("Variant '{}' has no cases", qualified_name.name),
                location: Position::new_start(),
                category: DiagnosticCategory::TypeError,
            });
        }

        // TODO: Validate case types exist and are accessible
        let _ = cases; // Suppress unused warning
    }

    /// Build constraint system for type checking
    fn build_constraint_system(
        &mut self,
        _namespace_tree: &NamespaceTree,
        _symbol_table: &SymbolTableBuilder,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement constraint system building
        // This would analyze function definitions and expressions to build
        // a system of type constraints that can be solved
    }

    /// Type check an expression
    #[allow(dead_code)]
    pub fn type_check_expression(
        &mut self,
        expression: &Exp,
        expected_type: Option<&NovaType>,
    ) -> TypeCheckResult {
        match expression {
            Exp::Number(_) => TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Integer)), // TODO: Distinguish int/float
            Exp::Bool(_) => TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Boolean)),
            Exp::String(_) => TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::String)),
            Exp::Nil(_) => TypeCheckResult::Success(NovaType::Unit),
            
            Exp::Lambda(exp_lambda) => {
                let lambda = &exp_lambda.lambda;
                // Type check lambda expression
                let mut param_types = Vec::new();
                
                for param in &lambda.params {
                    let param_type = if let Some(type_spec) = &param.type_spec {
                        self.convert_ast_type_to_nova_type(&type_spec.ty)
                    } else {
                        // Type inference needed - use error type as placeholder
                        NovaType::Error
                    };
                    param_types.push(param_type);
                }

                let return_type = self.convert_ast_type_to_nova_type(&lambda.return_type.ty);
                let function_type = NovaType::Function(param_types, Box::new(return_type));

                // Check against expected type if provided
                if let Some(expected) = expected_type {
                    if self.types_compatible(&function_type, expected) {
                        TypeCheckResult::Success(function_type)
                    } else {
                        TypeCheckResult::Error(TypeCheckError {
                            message: "Function type mismatch".to_string(),
                            expected: Some(expected.clone()),
                            actual: Some(function_type),
                            location: expression.span(),
                            suggestions: vec!["Check function signature".to_string()],
                        })
                    }
                } else {
                    TypeCheckResult::Success(function_type)
                }
            }

            Exp::Binary(b) => {
                // Type check binary expression
                let left_result = self.type_check_expression(&b.left, None);
                let right_result = self.type_check_expression(&b.right, None);

                match (left_result, right_result) {
                    (TypeCheckResult::Success(left_type), TypeCheckResult::Success(right_type)) => {
                        self.type_check_binary_operation(&left_type, &right_type, &b.op, expression.span())
                    }
                    (TypeCheckResult::Error(err), _) | (_, TypeCheckResult::Error(err)) => {
                        TypeCheckResult::Error(err)
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: "Cannot type check binary expression".to_string(),
                        expected: None,
                        actual: None,
                        location: expression.span(),
                        suggestions: Vec::new(),
                    })
                }
            }

            Exp::Unary(u) => {
                let operand_result = self.type_check_expression(&u.exp, None);
                match operand_result {
                    TypeCheckResult::Success(operand_type) => {
                        self.type_check_unary_operation(&operand_type, &u.op, expression.span())
                    }
                    TypeCheckResult::Error(err) => TypeCheckResult::Error(err),
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: "Cannot type check unary expression".to_string(),
                        expected: None,
                        actual: None,
                        location: expression.span(),
                        suggestions: Vec::new(),
                    })
                }
            }

            // Postfix / compound forms — full resolution deferred to later semantic passes
            Exp::Name(_)
            | Exp::Paren(_)
            | Exp::Field(_)
            | Exp::Index(_)
            | Exp::Call(_)
            | Exp::VarDecl(_) => {
                // TODO: resolve names, fields, indices, calls against the symbol table
                TypeCheckResult::Success(NovaType::Error)
            }
        }
    }

    /// Check if two types are compatible
    fn types_compatible(&self, type1: &NovaType, type2: &NovaType) -> bool {
        match (type1, type2) {
            (NovaType::Primitive(p1), NovaType::Primitive(p2)) => p1 == p2,
            (NovaType::Named(q1, args1), NovaType::Named(q2, args2)) => {
                q1 == q2 && args1.len() == args2.len() &&
                args1.iter().zip(args2.iter()).all(|(a1, a2)| self.types_compatible(a1, a2))
            }
            (NovaType::Function(params1, ret1), NovaType::Function(params2, ret2)) => {
                params1.len() == params2.len() &&
                params1.iter().zip(params2.iter()).all(|(p1, p2)| self.types_compatible(p1, p2)) &&
                self.types_compatible(ret1, ret2)
            }
            (NovaType::Unit, NovaType::Unit) => true,
            (NovaType::Error, _) | (_, NovaType::Error) => true, // Error type is compatible with anything
            _ => false,
        }
    }

    /// Type check binary operation
    fn type_check_binary_operation(
        &self,
        left_type: &NovaType,
        right_type: &NovaType,
        op: &crate::syntax::ast::BinOp,
        location: Span,
    ) -> TypeCheckResult {
        use crate::syntax::ast::BinOp;

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                match (left_type, right_type) {
                    (NovaType::Primitive(PrimitiveType::Integer), NovaType::Primitive(PrimitiveType::Integer)) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Integer))
                    }
                    (NovaType::Primitive(PrimitiveType::Float), NovaType::Primitive(PrimitiveType::Float)) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Float))
                    }
                    (NovaType::Primitive(PrimitiveType::String), NovaType::Primitive(PrimitiveType::String)) if *op == BinOp::Add => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::String))
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: format!("Cannot apply {:?} to types {:?} and {:?}", op, left_type, right_type),
                        expected: None,
                        actual: None,
                        location,
                        suggestions: vec!["Check operand types".to_string()],
                    })
                }
            }
            BinOp::Eq | BinOp::NotEq => {
                if self.types_compatible(left_type, right_type) {
                    TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Boolean))
                } else {
                    TypeCheckResult::Error(TypeCheckError {
                        message: format!("Cannot compare types {:?} and {:?}", left_type, right_type),
                        expected: None,
                        actual: None,
                        location,
                        suggestions: vec!["Ensure both operands have the same type".to_string()],
                    })
                }
            }
            BinOp::Less | BinOp::LessEq | BinOp::Greater | BinOp::GreaterEq => {
                match (left_type, right_type) {
                    (NovaType::Primitive(PrimitiveType::Integer), NovaType::Primitive(PrimitiveType::Integer)) |
                    (NovaType::Primitive(PrimitiveType::Float), NovaType::Primitive(PrimitiveType::Float)) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Boolean))
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: format!("Cannot compare types {:?} and {:?}", left_type, right_type),
                        expected: None,
                        actual: None,
                        location,
                        suggestions: vec!["Comparison requires numeric types".to_string()],
                    })
                }
            }
            BinOp::And | BinOp::Or => {
                match (left_type, right_type) {
                    (NovaType::Primitive(PrimitiveType::Boolean), NovaType::Primitive(PrimitiveType::Boolean)) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Boolean))
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: format!("Logical operations require boolean types, found {:?} and {:?}", left_type, right_type),
                        expected: Some(NovaType::Primitive(PrimitiveType::Boolean)),
                        actual: None,
                        location,
                        suggestions: vec!["Use boolean expressions".to_string()],
                    })
                }
            }
            _ => {
                // Handle other operators (bitwise, etc.)
                TypeCheckResult::Error(TypeCheckError {
                    message: format!("Type checking for operator {:?} not implemented", op),
                    expected: None,
                    actual: None,
                    location,
                    suggestions: vec!["Use supported operators".to_string()],
                })
            }
        }
    }

    /// Type check unary operation
    fn type_check_unary_operation(
        &self,
        operand_type: &NovaType,
        op: &crate::syntax::ast::UnOp,
        location: Span,
    ) -> TypeCheckResult {
        use crate::syntax::ast::UnOp;

        match op {
            UnOp::Not => {
                match operand_type {
                    NovaType::Primitive(PrimitiveType::Boolean) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Boolean))
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: format!("Cannot apply 'not' to type {:?}", operand_type),
                        expected: Some(NovaType::Primitive(PrimitiveType::Boolean)),
                        actual: Some(operand_type.clone()),
                        location,
                        suggestions: vec!["Use boolean expression".to_string()],
                    })
                }
            }
            UnOp::Neg => {
                match operand_type {
                    NovaType::Primitive(PrimitiveType::Integer) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Integer))
                    }
                    NovaType::Primitive(PrimitiveType::Float) => {
                        TypeCheckResult::Success(NovaType::Primitive(PrimitiveType::Float))
                    }
                    _ => TypeCheckResult::Error(TypeCheckError {
                        message: format!("Cannot apply unary minus to type {:?}", operand_type),
                        expected: None,
                        actual: Some(operand_type.clone()),
                        location,
                        suggestions: vec!["Use numeric type".to_string()],
                    })
                }
            }
            _ => {
                TypeCheckResult::Error(TypeCheckError {
                    message: format!("Type checking for unary operator {:?} not implemented", op),
                    expected: None,
                    actual: None,
                    location,
                    suggestions: vec!["Use supported operators".to_string()],
                })
            }
        }
    }

    /// Get public access to type environment
    #[allow(dead_code)]
    pub fn get_type_environment(&self) -> &TypeEnvironment {
        &self.type_environment
    }

    /// Get number of type definitions
    #[allow(dead_code)]
    pub fn get_type_count(&self) -> usize {
        self.type_environment.bundle_types.len()
    }

    /// Get number of primitive types
    #[allow(dead_code)]
    pub fn get_primitive_type_count(&self) -> usize {
        self.type_environment.primitive_types.len()
    }

    /// Get type definition by qualified name
    #[allow(dead_code)]
    pub fn get_type_definition(&self, qualified_name: &QualifiedName) -> Option<&TypeDefinition> {
        self.type_environment.bundle_types.get(qualified_name)
    }

    /// Check if type exists
    #[allow(dead_code)]
    pub fn type_exists(&self, qualified_name: &QualifiedName) -> bool {
        self.type_environment.bundle_types.contains_key(qualified_name)
    }

    /// Get primitive type info
    #[allow(dead_code)]
    pub fn get_primitive_type(&self, name: &str) -> Option<&PrimitiveTypeInfo> {
        self.type_environment.primitive_types.get(name)
    }

    /// Generate a fresh type variable
    #[allow(dead_code)]
    pub fn fresh_type_variable(&mut self) -> NovaType {
        let id = self.type_var_counter;
        self.type_var_counter += 1;
        NovaType::Variable(TypeVariable { id, name: None })
    }
}

impl std::fmt::Display for NovaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NovaType::Primitive(p) => write!(f, "{:?}", p),
            NovaType::Named(name, args) => {
                write!(f, "{}", name.name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            NovaType::Function(params, ret) => {
                write!(f, "(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", param)?;
                }
                write!(f, ") -> {}", ret)
            }
            NovaType::Trait(name, args) => {
                write!(f, "trait {}", name.name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            NovaType::Variable(var) => {
                if let Some(name) = &var.name {
                    write!(f, "'{}", name)
                } else {
                    write!(f, "'t{}", var.id)
                }
            }
            NovaType::Unit => write!(f, "unit"),
            NovaType::Error => write!(f, "<error>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::{Name, BinOp, UnOp};
    use crate::lexical::Position;

    #[test]
    fn test_type_system_creation() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name.clone());
        
        assert_eq!(type_system.bundle_name, bundle_name);
        assert_eq!(type_system.type_var_counter, 0);
        assert_eq!(type_system.active_constraints.len(), 0);
    }

    #[test]
    fn test_primitive_types_initialization() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name);
        
        assert!(type_system.type_environment.primitive_types.contains_key("integer"));
        assert!(type_system.type_environment.primitive_types.contains_key("float"));
        assert!(type_system.type_environment.primitive_types.contains_key("boolean"));
        assert!(type_system.type_environment.primitive_types.contains_key("string"));
        assert!(type_system.type_environment.primitive_types.contains_key("unit"));
    }

    #[test]
    fn test_type_compatibility() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name);
        
        let int_type = NovaType::Primitive(PrimitiveType::Integer);
        let float_type = NovaType::Primitive(PrimitiveType::Float);
        let unit_type = NovaType::Unit;
        
        assert!(type_system.types_compatible(&int_type, &int_type));
        assert!(!type_system.types_compatible(&int_type, &float_type));
        assert!(type_system.types_compatible(&unit_type, &unit_type));
    }

    #[test]
    fn test_fresh_type_variable() {
        let bundle_name = BundleName::from("test");
        let mut type_system = TypeSystem::new(bundle_name);
        
        let var1 = type_system.fresh_type_variable();
        let var2 = type_system.fresh_type_variable();
        
        match (&var1, &var2) {
            (NovaType::Variable(v1), NovaType::Variable(v2)) => {
                assert_ne!(v1.id, v2.id);
                assert_eq!(v1.id + 1, v2.id);
            }
            _ => panic!("Expected type variables"),
        }
    }

    #[test]
    fn test_convert_ast_type_to_nova_type() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name);
        
        let ast_type = TypeName {
            span: Span::single(Position::new_start()),
            parts: vec![Name {
                value: "integer".to_string(),
                span: Span::single(Position::new_start()),
            }],
        };
        
        let nova_type = type_system.convert_ast_type_to_nova_type(&ast_type);
        assert_eq!(nova_type, NovaType::Primitive(PrimitiveType::Integer));
    }

    #[test]
    fn test_type_display() {
        let int_type = NovaType::Primitive(PrimitiveType::Integer);
        let unit_type = NovaType::Unit;
        let func_type = NovaType::Function(
            vec![NovaType::Primitive(PrimitiveType::String)],
            Box::new(NovaType::Unit)
        );
        
        assert_eq!(format!("{}", int_type), "Integer");
        assert_eq!(format!("{}", unit_type), "unit");
        assert_eq!(format!("{}", func_type), "(String) -> unit");
    }

    #[test]
    fn test_binary_operation_type_checking() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name);
        
        let int_type = NovaType::Primitive(PrimitiveType::Integer);
        let bool_type = NovaType::Primitive(PrimitiveType::Boolean);
        let span = Span::single(Position::new_start());
        
        // Test arithmetic operation
        let result = type_system.type_check_binary_operation(&int_type, &int_type, &BinOp::Add, span);
        match result {
            TypeCheckResult::Success(result_type) => {
                assert_eq!(result_type, NovaType::Primitive(PrimitiveType::Integer));
            }
            _ => panic!("Expected successful type check"),
        }
        
        // Test logical operation
        let result = type_system.type_check_binary_operation(&bool_type, &bool_type, &BinOp::And, span);
        match result {
            TypeCheckResult::Success(result_type) => {
                assert_eq!(result_type, NovaType::Primitive(PrimitiveType::Boolean));
            }
            _ => panic!("Expected successful type check"),
        }
        
        // Test invalid operation
        let result = type_system.type_check_binary_operation(&int_type, &bool_type, &BinOp::Add, span);
        match result {
            TypeCheckResult::Error(_) => {
                // Expected
            }
            _ => panic!("Expected type check error"),
        }
    }

    #[test]
    fn test_unary_operation_type_checking() {
        let bundle_name = BundleName::from("test");
        let type_system = TypeSystem::new(bundle_name);
        
        let int_type = NovaType::Primitive(PrimitiveType::Integer);
        let bool_type = NovaType::Primitive(PrimitiveType::Boolean);
        let span = Span::single(Position::new_start());
        
        // Test logical NOT operation
        let result = type_system.type_check_unary_operation(&bool_type, &UnOp::Not, span);
        match result {
            TypeCheckResult::Success(result_type) => {
                assert_eq!(result_type, NovaType::Primitive(PrimitiveType::Boolean));
            }
            _ => panic!("Expected successful type check"),
        }
        
        // Test unary minus operation
        let result = type_system.type_check_unary_operation(&int_type, &UnOp::Neg, span);
        match result {
            TypeCheckResult::Success(result_type) => {
                assert_eq!(result_type, NovaType::Primitive(PrimitiveType::Integer));
            }
            _ => panic!("Expected successful type check"),
        }
        
        // Test invalid unary operation
        let result = type_system.type_check_unary_operation(&bool_type, &UnOp::Neg, span);
        match result {
            TypeCheckResult::Error(_) => {
                // Expected
            }
            _ => panic!("Expected type check error"),
        }
    }
}
