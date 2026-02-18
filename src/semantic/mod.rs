//! Nova Semantic Analysis System
//! 
//! This module implements the semantic analysis phase of the Nova compiler,
//! transforming parsed AST into a fully resolved semantic model with type
//! checking, symbol resolution, and cross-bundle linking.

pub mod bundle;
pub mod namespace;
pub mod symbols;

use crate::syntax::ast::Chunk;
use crate::lexical::token::Position;
use std::collections::HashMap;

/// Primary semantic analysis result containing all resolved semantic information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SemanticModel {
    /// The analyzed bundle with all dependencies resolved
    pub bundle: bundle::Bundle,
    /// Namespace tree with resolved hierarchy
    pub namespace_tree: namespace::NamespaceTree,
    /// Global symbol table for cross-bundle resolution
    pub symbol_table: GlobalSymbolTable,
    /// Type environment with all resolved types
    pub type_environment: TypeEnvironment,
    /// Collected semantic errors and warnings
    pub diagnostics: Vec<SemanticDiagnostic>,
}

/// Global symbol table managing symbols across all bundles
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct GlobalSymbolTable {
    /// Exported symbols from all analyzed bundles
    pub exported_symbols: HashMap<QualifiedName, ExportedSymbol>,
    /// Imported symbols and their resolution status
    pub imported_symbols: HashMap<(bundle::BundleName, QualifiedName), ImportedSymbol>,
    /// Detected symbol conflicts
    pub symbol_conflicts: Vec<SymbolConflict>,
}

/// Type environment containing all type information
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct TypeEnvironment {
    /// Type definitions from all bundles
    pub bundle_types: HashMap<QualifiedName, TypeDefinition>,
    /// Imported type references
    pub imported_types: HashMap<String, TypeReference>,
    /// Type aliases
    pub type_aliases: HashMap<String, Type>,
}

/// Fully qualified name for global symbol identification
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct QualifiedName {
    /// Bundle name containing the symbol
    pub bundle: bundle::BundleName,
    /// Namespace path within the bundle
    pub namespace: Vec<String>,
    /// Local name of the symbol
    pub name: String,
}

/// Exported symbol available to other bundles
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ExportedSymbol {
    /// The actual definition
    pub definition: Definition,
    /// Bundle that exports this symbol
    pub source_bundle: bundle::BundleName,
    /// Visibility constraints
    pub visibility_constraints: Vec<VisibilityConstraint>,
    /// Mangled name for linking
    pub mangled_name: String,
}

/// Imported symbol and its resolution status
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ImportedSymbol {
    /// Original qualified name being imported
    pub original_symbol: QualifiedName,
    /// Source bundle providing the symbol
    pub source_bundle: bundle::BundleName,
    /// Local name in importing context
    pub local_name: String,
    /// Current resolution status
    pub resolution_status: ResolutionStatus,
}

/// Symbol resolution status
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum ResolutionStatus {
    /// Successfully resolved to an exported symbol
    Resolved(ExportedSymbol),
    /// Failed to resolve for a specific reason
    Unresolved(UnresolvedReason),
    /// Multiple candidates found
    Ambiguous(Vec<ExportedSymbol>),
}

/// Reason why a symbol couldn't be resolved
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum UnresolvedReason {
    /// Symbol not found in any available bundle
    SymbolNotFound,
    /// Symbol found but not accessible due to visibility rules
    VisibilityRestriction,
    /// Version constraints couldn't be satisfied
    VersionMismatch,
    /// Circular dependency preventing resolution
    CircularDependency,
}

/// Symbol conflict between multiple definitions
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SymbolConflict {
    /// The conflicting symbol name
    pub symbol: QualifiedName,
    /// All conflicting definitions
    pub conflicting_symbols: Vec<ExportedSymbol>,
    /// Source location where conflict was detected
    pub location: Position,
}

/// Visibility constraint on symbol access
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct VisibilityConstraint {
    /// The visibility level required
    pub required_visibility: Visibility,
    /// Bundles that have access (if restricted)
    pub allowed_bundles: Option<Vec<bundle::BundleName>>,
}

/// Visibility levels for symbols
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Visibility {
    /// Only visible within the same namespace
    Private,
    /// Visible within the same bundle
    BundlePrivate,
    /// Publicly visible to all bundles
    Public,
    /// Visible only to specific bundles
    Restricted(Vec<bundle::BundleName>),
}

/// Type definition in the semantic model
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum TypeDefinition {
    /// Struct type with fields
    Struct {
        fields: HashMap<String, (Type, FieldMetadata)>,
        type_parameters: Vec<TypeParameter>,
    },
    /// Enum type with variants
    Enum {
        base_type: Type,
        variants: HashMap<String, i64>,
        type_parameters: Vec<TypeParameter>,
    },
    /// Variant type with cases
    Variant {
        cases: HashMap<String, Type>,
        type_parameters: Vec<TypeParameter>,
    },
    /// Trait type with method signatures
    Trait {
        signatures: HashMap<String, FunctionSignature>,
        type_parameters: Vec<TypeParameter>,
    },
    /// Type alias
    Alias {
        target: Type,
        type_parameters: Vec<TypeParameter>,
    },
}

/// Type reference to another type
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeReference {
    /// Qualified name of the referenced type
    pub qualified_name: QualifiedName,
    /// Type arguments if generic
    pub type_arguments: Vec<Type>,
}

/// Type in the type system
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Type {
    /// Primitive types (integer, boolean, etc.)
    Primitive(PrimitiveType),
    /// Named type with optional type arguments
    Named(QualifiedName, Vec<Type>),
    /// Function type
    Function(Vec<Type>, Box<Type>),
    /// Trait object type
    Trait(QualifiedName, Vec<Type>),
}

/// Primitive type kinds
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PrimitiveType {
    Integer,
    Float,
    Boolean,
    String,
    Unit,
}

/// Type parameter for generic types
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TypeParameter {
    /// Parameter name
    pub name: String,
    /// Trait bounds
    pub bounds: Vec<Type>,
}

/// Field metadata
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FieldMetadata {
    /// Field visibility
    pub visibility: Visibility,
    /// Whether field is mutable
    pub is_mutable: bool,
}

/// Function signature
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FunctionSignature {
    /// Function name
    pub name: String,
    /// Type parameters
    pub type_parameters: Vec<TypeParameter>,
    /// Parameters
    pub parameters: Vec<Parameter>,
    /// Return type
    pub return_type: Type,
    /// Whether function is const
    pub is_const: bool,
}

/// Function parameter
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub param_type: Type,
}

/// Definition in the semantic model
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Definition {
    /// Type definition
    Type {
        type_def: TypeDefinition,
        visibility: Visibility,
    },
    /// Value definition
    Value {
        value_type: Type,
        visibility: Visibility,
        is_mutable: bool,
    },
    /// Function definition
    Function {
        signature: FunctionSignature,
        visibility: Visibility,
    },
}

/// Semantic diagnostic (error or warning)
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SemanticDiagnostic {
    /// Error severity
    pub severity: DiagnosticSeverity,
    /// Error message
    pub message: String,
    /// Source location
    pub location: Position,
    /// Diagnostic category
    pub category: DiagnosticCategory,
}

/// Diagnostic severity levels
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// Diagnostic categories
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DiagnosticCategory {
    TypeError,
    SymbolResolution,
    VisibilityViolation,
    DependencyError,
    CoherenceConflict,
}

/// Main entry point for semantic analysis
pub fn analyze_bundle(chunks: Vec<Chunk>) -> Result<SemanticModel, Vec<SemanticDiagnostic>> {
    let mut diagnostics = Vec::new();
    
    // Step 1: Create bundle from chunks
    let bundle = bundle::Bundle::from_chunks(chunks.clone(), &mut diagnostics)?;
    
    // Step 2: Build namespace tree
    let namespace_tree = namespace::NamespaceTree::from_compilation_units(
        bundle.name.clone(),
        &chunks,
        &mut diagnostics,
    );
    
    // Step 3: Build symbol tables and resolve cross-references
    let symbol_table = GlobalSymbolTable {
        exported_symbols: symbols::SymbolTableBuilder::build_from_namespace_tree(
            bundle.name.clone(),
            &namespace_tree,
            &mut diagnostics,
        ),
        imported_symbols: HashMap::new(), // Will be populated during cross-bundle linking
        symbol_conflicts: Vec::new(),
    };
    
    // Validate namespace tree (temporarily disabled to debug hanging)
    // namespace_tree.validate(&mut diagnostics);
    
    let model = SemanticModel {
        bundle,
        namespace_tree,
        symbol_table,
        type_environment: TypeEnvironment::default(),
        diagnostics: diagnostics.clone(),
    };
    
    if diagnostics.is_empty() {
        Ok(model)
    } else {
        // For now, return errors if any diagnostics exist
        // In the future, we may want to return warnings but continue
        Err(diagnostics)
    }
}