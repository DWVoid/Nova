# Nova Semantic Model

This document defines the semantic model for Nova bundles - collections of compilation units that can have dependencies and are assembled and linked together.

## 1. Bundle Structure

A **Bundle** is the primary unit of compilation, distribution, and linking in Nova. It consists of:

```
Bundle ::= {
    name: BundleName,
    version: Version,
    compilation_units: [CompilationUnit],
    dependencies: [BundleDependency],
    exports: NamespaceExports,
    metadata: BundleMetadata
}
```

### 1.1 Bundle Identity

```
BundleName ::= QualifiedName       // e.g., "com.example.mylib"
Version ::= SemVer                 // Semantic versioning: "1.2.3"

BundleDependency ::= {
    name: BundleName,
    version_constraint: VersionConstraint,
    visibility: DependencyVisibility
}

VersionConstraint ::= Exact(Version)
                   | Range(Version, Version)
                   | Compatible(Version)     // ^1.2.3 means >=1.2.3 <2.0.0

DependencyVisibility ::= Public    // Re-exported to bundle users
                      | Private    // Internal use only
```

## 2. Namespace Resolution and Exports

### 2.1 Namespace Hierarchy

Each compilation unit declares its namespace, creating a hierarchical structure within the bundle:

```
NamespaceTree ::= {
    root: BundleName,
    namespaces: Map<NamespacePath, NamespaceScope>
}

NamespacePath ::= [String]         // e.g., ["System", "Collections", "List"]

NamespaceScope ::= {
    definitions: Map<String, Definition>,
    implementations: [Implementation],
    nested_namespaces: Map<String, NamespaceScope>,
    imports: [ResolvedImport]
}
```

### 2.2 Export Resolution

```
NamespaceExports ::= {
    public_namespaces: Set<NamespacePath>,
    exported_definitions: Map<QualifiedName, ExportedDefinition>
}

ExportedDefinition ::= {
    definition: Definition,
    visibility_scopes: Option<[BundleName]>,  // Limited export scopes
    original_namespace: NamespacePath
}
```

### 2.3 Import Resolution

```
ResolvedImport ::= {
    source_bundle: BundleName,
    source_namespace: NamespacePath,
    imported_items: [ImportedItem],
    resolution_time: ImportTime
}

ImportedItem ::= DirectImport {
                    original_name: String,
                    local_name: String,
                    definition_ref: DefinitionReference
                 }
               | SelectorImport {
                    items: Map<String, String>,  // original_name -> local_name
                    definition_refs: Map<String, DefinitionReference>
                 }

ImportTime ::= CompileTime     // Resolved during compilation
            | LinkTime        // Resolved during bundle linking

DefinitionReference ::= {
    bundle: BundleName,
    namespace: NamespacePath,
    name: String,
    definition_kind: DefinitionKind
}
```

## 3. Type System

### 3.1 Type Resolution

```
TypeEnvironment ::= {
    bundle_types: Map<QualifiedName, TypeDefinition>,
    imported_types: Map<String, TypeReference>,
    type_aliases: Map<String, Type>,
    trait_implementations: Map<(Type, TraitType), Implementation>
}

Type ::= PrimitiveType(PrimitiveKind)
      | NamedType(QualifiedName, [TypeArgument])
      | FunctionType([Type], Type)
      | TraitType(QualifiedName, [TypeArgument])

TypeDefinition ::= StructType {
                      fields: Map<String, (Type, FieldMetadata)>,
                      type_parameters: [TypeParameter]
                   }
                 | EnumType {
                      base_type: Type,
                      variants: Map<String, Value>,
                      type_parameters: [TypeParameter]
                   }
                 | VariantType {
                      cases: Map<String, Type>,
                      type_parameters: [TypeParameter]
                   }
                 | TraitType {
                      signatures: Map<String, FunctionSignature>,
                      type_parameters: [TypeParameter]
                   }
                 | TypeAlias {
                      target: Type,
                      type_parameters: [TypeParameter]
                   }
```

### 3.2 Type Checking Context

```
TypeContext ::= {
    current_namespace: NamespacePath,
    current_bundle: BundleName,
    visible_types: Map<String, Type>,
    visible_functions: Map<String, FunctionSignature>,
    type_constraints: [TypeConstraint],
    lambda_environment: LambdaEnvironment
}

TypeConstraint ::= TraitBound(TypeVariable, TraitType)
                | EqualityConstraint(Type, Type)
                | SubtypeConstraint(Type, Type)
```

## 4. Definition Semantics

### 4.1 Definition Environment

```
DefinitionEnvironment ::= {
    current_scope: DefinitionScope,
    parent_scopes: [DefinitionScope],
    decorators: [ResolvedDecorator]
}

DefinitionScope ::= BundleScope {
                       bundle_name: BundleName,
                       exported_definitions: Map<String, Definition>
                   }
                 | NamespaceScope {
                       namespace_path: NamespacePath,
                       local_definitions: Map<String, Definition>
                   }
                 | BlockScope {
                       local_variables: Map<String, Variable>,
                       captured_variables: Map<String, CapturedVariable>
                   }
```

### 4.2 Definition Kinds

```
Definition ::= TypeDefinition {
                  type_def: TypeDefinition,
                  visibility: Visibility,
                  decorators: [Decorator]
              }
             | ValueDefinition {
                  value: Value,
                  value_type: Type,
                  mutability: Mutability,
                  visibility: Visibility,
                  decorators: [Decorator]
              }
             | FunctionDefinition {
                  signature: FunctionSignature,
                  implementation: FunctionBody,
                  visibility: Visibility,
                  decorators: [Decorator]
              }

FunctionSignature ::= {
    name: String,
    type_parameters: [TypeParameter],
    parameters: [Parameter],
    return_type: Type,
    constraints: [TypeConstraint],
    is_const: Bool
}

FunctionBody ::= LambdaBody {
                    parameters: [Parameter],
                    body: Block,
                    captured_environment: CaptureEnvironment
                }
              | ExternBody {
                    external_name: String,
                    calling_convention: CallingConvention
                }
```

## 5. Implementation and Trait Semantics

### 5.1 Implementation Resolution

```
ImplementationTable ::= {
    trait_implementations: Map<(Type, TraitType), TraitImplementation>,
    inherent_implementations: Map<Type, [InherentImplementation]>,
    coherence_graph: CoherenceGraph
}

TraitImplementation ::= {
    target_type: Type,
    trait_type: TraitType,
    method_implementations: Map<String, FunctionDefinition>,
    associated_types: Map<String, Type>,
    constraints: [TypeConstraint],
    coherence_conditions: [CoherenceCondition]
}

CoherenceGraph ::= {
    nodes: Set<(Type, TraitType)>,
    edges: Set<((Type, TraitType), (Type, TraitType))>,
    conflicts: [CoherenceConflict]
}
```

### 5.2 Method Resolution

```
MethodResolution ::= {
    candidate_methods: [MethodCandidate],
    resolution_order: [ResolutionStep],
    selected_method: Option<ResolvedMethod>
}

MethodCandidate ::= InherentMethod {
                       target_type: Type,
                       method: FunctionDefinition
                   }
                 | TraitMethod {
                       target_type: Type,
                       trait_type: TraitType,
                       method: FunctionDefinition
                   }

ResolutionStep ::= TypeUnification(Type, Type)
                | TraitBoundCheck(Type, TraitType)
                | VisibilityCheck(Definition, AccessContext)
                | CoherenceCheck(TraitImplementation)
```

## 6. Bundle Linking and Assembly

### 6.1 Link-Time Resolution

```
LinkContext ::= {
    available_bundles: Map<BundleName, Bundle>,
    dependency_graph: DependencyGraph,
    symbol_table: GlobalSymbolTable,
    version_resolution: VersionResolver
}

DependencyGraph ::= {
    nodes: Set<BundleName>,
    edges: Map<BundleName, Set<BundleDependency>>,
    resolution_order: [BundleName],
    circular_dependencies: [CircularDependency]
}

GlobalSymbolTable ::= {
    exported_symbols: Map<QualifiedName, ExportedSymbol>,
    imported_symbols: Map<(BundleName, QualifiedName), ImportedSymbol>,
    symbol_conflicts: [SymbolConflict]
}
```

### 6.2 Symbol Resolution

```
ExportedSymbol ::= {
    definition: Definition,
    source_bundle: BundleName,
    visibility_constraints: [VisibilityConstraint],
    mangled_name: String
}

ImportedSymbol ::= {
    original_symbol: QualifiedName,
    source_bundle: BundleName,
    local_name: String,
    resolution_status: ResolutionStatus
}

ResolutionStatus ::= Resolved(ExportedSymbol)
                   | Unresolved(UnresolvedReason)
                   | Ambiguous([ExportedSymbol])

UnresolvedReason ::= SymbolNotFound
                   | VisibilityRestriction
                   | VersionMismatch
                   | CircularDependency
```

## 7. Visibility and Access Control

### 7.1 Visibility Rules

```
Visibility ::= Private              // Only within same namespace
             | BundlePrivate        // Only within same bundle
             | Public               // Accessible to all bundles
             | Restricted([BundleName])  // Limited to specific bundles

AccessContext ::= {
    accessing_bundle: BundleName,
    accessing_namespace: NamespacePath,
    access_kind: AccessKind
}

AccessKind ::= TypeAccess
            | ValueAccess
            | MethodCall
            | FieldAccess
            | TraitImplementation
```

### 7.2 Access Resolution

```
AccessCheck ::= {
    requested_access: (Definition, AccessContext),
    visibility_chain: [VisibilityScope],
    access_result: AccessResult
}

AccessResult ::= Granted
               | Denied(DenialReason)

DenialReason ::= PrivateAccess
               | BundleRestriction
               | ScopeRestriction
               | DeprecationWarning
```

## 8. Decorator Semantics

### 8.1 Decorator Resolution

```
ResolvedDecorator ::= {
    decorator_function: FunctionDefinition,
    arguments: [Value],
    target_kind: DecoratorTarget,
    expansion_result: DecoratorResult
}

DecoratorTarget ::= TypeTarget(TypeDefinition)
                 | FunctionTarget(FunctionDefinition)
                 | FieldTarget(FieldDefinition)
                 | BundleTarget(Bundle)

DecoratorResult ::= CodeGeneration([Statement])
                 | MetadataAddition(Metadata)
                 | CompileTimeExecution(Value)
                 | ValidationConstraint(Constraint)
```

## 9. Error Handling and Diagnostics

### 9.1 Semantic Errors

```
SemanticError ::= TypeMismatch {
                     expected: Type,
                     actual: Type,
                     context: TypeContext
                 }
                | UnresolvedSymbol {
                     symbol: String,
                     available_symbols: [String],
                     context: AccessContext
                 }
                | VisibilityViolation {
                     definition: Definition,
                     access_context: AccessContext,
                     required_visibility: Visibility
                 }
                | CircularDependency {
                     dependency_chain: [BundleName]
                 }
                | CoherenceConflict {
                     conflicting_implementations: [TraitImplementation]
                 }
                | AmbiguousImport {
                     symbol: String,
                     candidates: [DefinitionReference]
                 }
```

### 9.2 Diagnostic Context

```
DiagnosticContext ::= {
    source_location: SourceLocation,
    semantic_context: SemanticContext,
    error_recovery: ErrorRecovery
}

SemanticContext ::= {
    current_phase: CompilationPhase,
    bundle_context: BundleName,
    namespace_context: NamespacePath,
    definition_context: Option<Definition>
}

CompilationPhase ::= Parsing
                  | SemanticAnalysis
                  | TypeChecking
                  | LinkTimeResolution
                  | CodeGeneration
```

## 10. Bundle Metadata and Versioning

### 10.1 Bundle Configuration

```
BundleMetadata ::= {
    manifest_version: String,
    authors: [String],
    description: String,
    license: String,
    repository: Option<String>,
    keywords: [String],
    categories: [String],
    build_configuration: BuildConfig,
    feature_flags: Map<String, Bool>
}

BuildConfig ::= {
    target_platform: Platform,
    optimization_level: OptimizationLevel,
    debug_symbols: Bool,
    custom_build_steps: [BuildStep]
}
```

### 10.2 Version Compatibility

```
VersionCompatibility ::= {
    api_version: Version,
    abi_version: Version,
    compatibility_matrix: Map<Version, CompatibilityLevel>
}

CompatibilityLevel ::= FullCompatible      // Source and binary compatible
                    | SourceCompatible     // Requires recompilation
                    | BreakingChange       // Manual migration needed
```

This semantic model provides the foundation for implementing Nova's module system, type checking, linking, and compilation processes while maintaining clear separation between bundle boundaries and proper visibility controls.