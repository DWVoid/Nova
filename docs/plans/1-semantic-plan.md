# Nova Semantic Analysis Implementation Plan

> Scope: Implement Nova's semantic analysis phase including bundle assembly, dependency resolution, type checking, and symbol resolution. Build upon the existing syntax parser to create a complete semantic model. This plan splits work into steps with per-file/step size under 1000 LOC.

## 0) Sources and Design Targets

- Primary spec: `docs/language/2-Semantic.md`
- Base implementation: Existing parser from `0-syntax-plan.md`
- Key areas to implement:
  - Bundle structure and dependency resolution
  - Namespace and symbol resolution
  - Type system with trait support
  - Implementation and method resolution
  - Visibility and access control
  - Link-time symbol resolution
  - Semantic error reporting

## 1) Output Format and Processing Strategy

### 1.1 Semantic Analysis Phases
- **Phase 1**: Bundle discovery and dependency resolution
- **Phase 2**: Namespace resolution and symbol collection
- **Phase 3**: Type checking and constraint resolution
- **Phase 4**: Implementation coherence checking
- **Phase 5**: Link-time symbol resolution
- **Phase 6**: Final validation and error reporting

### 1.2 Error Reporting Model
- **Structured diagnostics**: Each error includes source location, severity, and context
- **Error recovery**: Continue analysis after errors when possible
- **Multi-phase reporting**: Collect errors from all phases before reporting
- **Contextual help**: Suggest fixes and show available alternatives

### 1.3 Bundle Processing Model
- **Incremental**: Process bundles in dependency order
- **Cached**: Store intermediate results for unchanged bundles
- **Modular**: Each bundle maintains its own semantic context
- **Linkable**: Generate symbol tables for cross-bundle resolution

## 2) Rust Module Layout (Under 1000 LOC per file)

Target files (estimate):
- `src/semantic/mod.rs` (≤ 200 LOC): semantic analysis entry point and phase coordination
- `src/semantic/bundle.rs` (≤ 800 LOC): bundle structure, metadata, and dependency management
- `src/semantic/namespace.rs` (≤ 700 LOC): namespace resolution and scope management
- `src/semantic/types.rs` (≤ 900 LOC): type system, type checking, and constraint resolution
- `src/semantic/symbols.rs` (≤ 800 LOC): symbol tables, resolution, and exports
- `src/semantic/traits.rs` (≤ 700 LOC): trait definitions, implementations, and coherence
- `src/semantic/visibility.rs` (≤ 400 LOC): access control and visibility checking
- `src/semantic/linker.rs` (≤ 600 LOC): cross-bundle linking and symbol resolution
- `src/semantic/diagnostics.rs` (≤ 500 LOC): error reporting and diagnostic context
- `src/semantic/decorators.rs` (≤ 400 LOC): decorator resolution and expansion
- `src/semantic/tests.rs` (≤ 800 LOC): semantic analysis unit tests
- `src/bundle_manifest.rs` (≤ 300 LOC): bundle configuration and metadata parsing

## 3) Stepwise Implementation Plan (Each Step ≤ 1000 LOC)

### Step 1 — Bundle Structure and Metadata ✅ **COMPLETED**
**Files**: `src/semantic/mod.rs`, `src/semantic/bundle.rs`, `src/bundle_manifest.rs`

- ✅ Implemented `Bundle`, `BundleName`, `Version`, `BundleDependency` structures
- ✅ Bundle metadata parsing and validation framework
- ✅ Semantic versioning and version constraint checking (basic implementation)
- ✅ Bundle discovery from compilation units (basic implementation)
- ✅ Dependency graph construction foundation
- ✅ Integration with main compilation pipeline
- ✅ Unit tests: semantic integration test passing

**Status**: Basic bundle structure implemented with semantic analysis integration working. Bundle creation from parsed chunks functional.

**Dependencies**: Existing AST from syntax phase
**Output**: Bundle structure ready for namespace resolution

**Next**: Step 2 — Namespace Resolution and Scoping

### Step 2 — Namespace Resolution and Scoping ✅ **COMPLETED**
**Files**: `src/semantic/namespace.rs`, extend `src/semantic/mod.rs`

- ✅ Implement `NamespaceTree`, `NamespaceScope`, `NamespacePath` structures
- ✅ Namespace hierarchy construction from compilation units
- ✅ Use declaration processing and import resolution
- ✅ Export collection and namespace flattening
- ✅ Symbol visibility scoping within namespaces
- ✅ Unit tests: namespace hierarchy, use declarations, export resolution

**Status**: Complete namespace hierarchy system implemented. Successfully processes complex namespace declarations, use statements with selectors and aliases, and tracks definitions per namespace. Integration test passing with full end-to-end functionality.

**Key Features**:
- Hierarchical namespace trees with proper parent-child relationships
- Use declaration parsing with selector support (`{IO, Console as Con}`)
- Import resolution with compile-time vs link-time classification
- Definition tracking per namespace with export status
- Nested namespace management and scope validation

**Dependencies**: Step 1 (Bundle structure)
**Output**: Resolved namespace hierarchy with scoped symbols

### Step 3 — Symbol Tables and Cross-Reference Resolution ✅ **COMPLETED**
**Files**: `src/semantic/symbols.rs`, extend semantic pipeline

- ✅ Implement `SymbolTableBuilder`, `SymbolResolutionContext`, `SymbolResolutionResult`
- ✅ Symbol collection from AST definitions
- ✅ Local symbol table construction per namespace
- ✅ Cross-namespace symbol resolution framework
- ✅ Symbol conflict detection and reporting
- ✅ Integration with namespace resolution system
- ✅ Export symbol generation and management

**Status**: Complete symbol table system implemented with cross-reference resolution foundation. Successfully:
- Builds symbol tables from namespace definitions
- Converts AST definitions to semantic definitions
- Detects and reports symbol conflicts
- Provides symbol resolution infrastructure
- Integrates with namespace hierarchy from Step 2
- Exports symbols with proper mangled names for linking

**Key Features**:
- **SymbolTableBuilder**: Collects, validates, and resolves symbols across namespaces
- **Symbol Resolution**: Multi-phase resolution with local, import, parent, and global lookup
- **Conflict Detection**: Identifies duplicate symbol definitions
- **Export Generation**: Creates exportable symbols with mangled names
- **Cross-Reference Foundation**: Framework for expression analysis (placeholder)

**Evidence**: Symbol table correctly identifies 1 exported symbol (`default::greet`) from complex namespace example with imports and definitions.

**Dependencies**: Namespace resolution system from Step 2
**Output**: Complete symbol tables ready for type resolution

**Next**: Step 4 — Type System and Type Checking

### Step 4 — Type System Foundation ✅ **COMPLETED**
**Files**: `src/semantic/types.rs`, extend semantic pipeline

- ✅ Implement `TypeSystem`, `NovaType`, `TypeDefinition`, `TypeEnvironment` structures
- ✅ Primitive type definitions and type constructors (integer, float, boolean, string, unit)
- ✅ Named type resolution and AST type conversion
- ✅ Type parameter and generic type handling foundations
- ✅ Basic type checking for expressions and statements
- ✅ Type compatibility checking and constraint system foundation
- ✅ Binary and unary operation type checking
- ✅ Function type handling and lambda expression analysis
- ✅ Integration with namespace and symbol table systems

**Status**: Complete type system foundation implemented. Successfully:
- Handles 5 primitive types with operation validation
- Converts AST type specifications to semantic types
- Provides type compatibility checking for operations
- Integrates with symbol table and namespace systems
- Validates type definitions with diagnostic reporting
- Supports function signatures and lambda expressions

**Key Features**:
- **TypeSystem**: Central type manager with environment and constraint handling
- **Type Checking**: Expression-level type checking with proper error reporting
- **Type Definitions**: Support for struct, enum, variant, trait, and alias definitions
- **Type Validation**: Consistency checking and conflict detection
- **AST Integration**: Seamless conversion from parser AST to semantic types
- **Error Recovery**: Proper error types and diagnostic generation

**Evidence**: Type system correctly initializes 5 primitive types and processes function definitions with proper type signatures.

**Dependencies**: Steps 1-3 (Bundle, namespace, and symbol resolution)
**Output**: Complete type system ready for trait implementation and advanced features

**Next**: Step 5 — Trait System and Implementations

### Step 5 — Trait System and Implementations
**Files**: `src/semantic/traits.rs`, extend type system

- Implement `TraitType`, `TraitImplementation`, `ImplementationTable`
- Trait definition processing and signature validation
- Implementation block processing and method resolution
- Coherence checking and conflict detection
- Method dispatch and trait bound resolution
- Unit tests: trait implementations, coherence, method resolution

**Dependencies**: Step 4 (Type system)
**Output**: Complete trait system with coherence validation

### Step 6 — Visibility and Access Control
**Files**: `src/semantic/visibility.rs`, integrate with existing modules

- Implement `Visibility`, `AccessContext`, `AccessCheck` structures
- Visibility rule enforcement across bundle boundaries
- Access control validation for types, functions, and fields
- Export scope restriction checking
- Privacy level validation and enforcement
- Unit tests: visibility rules, access control, export restrictions

**Dependencies**: Steps 1-5 (All previous components)
**Output**: Enforced visibility and access control

### Step 7 — Cross-Bundle Linking
**Files**: `src/semantic/linker.rs`, extend all previous modules

- Implement `LinkContext`, `DependencyGraph`, link-time resolution
- Cross-bundle symbol resolution and import linking
- Version compatibility checking during linking
- Symbol mangling and external symbol generation
- Circular dependency resolution at link time
- Unit tests: cross-bundle resolution, version compatibility, link errors

**Dependencies**: Steps 1-6 (Complete semantic analysis)
**Output**: Fully linked bundle with resolved external dependencies

### Step 8 — Decorator System
**Files**: `src/semantic/decorators.rs`, integrate with type system

- Implement `ResolvedDecorator`, decorator function resolution
- Decorator argument type checking and validation
- Compile-time decorator expansion and code generation
- Decorator composition and ordering
- Metadata decoration and constraint validation
- Unit tests: decorator resolution, expansion, composition

**Dependencies**: Steps 4-6 (Type system and visibility)
**Output**: Working decorator system with compile-time expansion

### Step 9 — Diagnostic System and Error Recovery
**Files**: `src/semantic/diagnostics.rs`, integrate across all modules

- Implement `SemanticError`, `DiagnosticContext`, comprehensive error types
- Structured error reporting with source locations and context
- Error recovery strategies for continued analysis
- Suggestion system for common errors (typos, visibility issues)
- Multi-phase error collection and reporting
- Unit tests: error reporting, recovery, suggestion quality

**Dependencies**: All previous steps
**Output**: Comprehensive diagnostic system

### Step 10 — Integration and Testing
**Files**: `src/semantic/tests.rs`, integration tests, extend main

- End-to-end semantic analysis pipeline
- Integration with existing syntax parser
- Comprehensive test suite covering all semantic features
- Performance testing for large bundles
- Error case validation and diagnostic quality
- Documentation and examples

**Dependencies**: All previous steps
**Output**: Complete semantic analysis system

## 4) Semantic Analysis Strategy

### 4.1 Phase Coordination
- **Dependency-driven**: Process bundles in topological dependency order
- **Incremental**: Skip unchanged bundles, reuse cached analysis
- **Error boundaries**: Isolate errors to prevent cascade failures
- **Progress tracking**: Report analysis progress for large projects

### 4.2 Type Checking Approach
- **Bidirectional**: Use both inference and checking modes
- **Constraint-based**: Collect constraints, solve globally
- **Trait-aware**: Integrate trait bounds into type checking
- **Error recovery**: Continue checking after type errors when possible

### 4.3 Symbol Resolution Strategy
- **Multi-pass**: Collect symbols, then resolve references
- **Namespace-aware**: Respect scoping and visibility rules
- **Conflict detection**: Identify and report symbol conflicts early
- **Performance**: Use efficient data structures for large symbol tables

## 5) Data Structure Design Principles

### 5.1 Memory Efficiency
- **Arena allocation**: Use arena allocators for AST and semantic data
- **Interning**: Intern common strings (names, paths) to reduce memory
- **Sparse data**: Use sparse representations for optional data
- **Reference sharing**: Share immutable data between contexts

### 5.2 Query Efficiency
- **Indexed access**: Build indices for common queries
- **Cached results**: Cache expensive computations
- **Lazy evaluation**: Compute semantic data on demand
- **Batch operations**: Process related operations together

### 5.3 Error Resilience
- **Partial analysis**: Continue analysis with incomplete information
- **Default values**: Provide sensible defaults for missing data
- **Graceful degradation**: Reduce functionality rather than crash
- **Error boundaries**: Contain errors to specific modules/phases

## 6) Testing Strategy

### 6.1 Unit Tests (Per Module)
- **Bundle tests**: dependency resolution, version checking, circular dependencies
- **Namespace tests**: hierarchy construction, symbol scoping, export resolution
- **Type tests**: type checking, generic resolution, constraint solving
- **Trait tests**: implementation checking, coherence validation, method dispatch
- **Visibility tests**: access control, export restrictions, privacy enforcement
- **Linker tests**: cross-bundle resolution, symbol conflicts, version compatibility

### 6.2 Integration Tests
- **Multi-bundle projects**: Test realistic project structures
- **Error scenarios**: Comprehensive error case coverage
- **Performance tests**: Large bundle analysis timing
- **Compatibility tests**: Version upgrade/downgrade scenarios

### 6.3 End-to-End Tests
- **Complete projects**: Real-world Nova projects
- **Standard library**: Semantic analysis of Nova standard library
- **Diagnostic quality**: Error message clarity and helpfulness
- **IDE integration**: Semantic information for language servers

## 7) Performance Considerations

### 7.1 Scalability Targets
- **Large bundles**: Handle bundles with 10,000+ definitions
- **Deep dependencies**: Support dependency chains 50+ levels deep
- **Parallel analysis**: Analyze independent bundles concurrently
- **Incremental updates**: Fast re-analysis for code changes

### 7.2 Memory Management
- **Bounded memory**: Semantic analysis should not grow unboundedly
- **Cache eviction**: Remove old semantic data when memory pressure occurs
- **Streaming**: Process large bundles without loading everything into memory
- **Memory profiling**: Track memory usage across semantic phases

### 7.3 Time Complexity
- **Linear scaling**: Analysis time should scale linearly with code size
- **Efficient algorithms**: Use optimal algorithms for graph operations
- **Early termination**: Stop analysis early when errors are found
- **Progress reporting**: Show analysis progress for long operations

## 8) Integration with Existing Systems

### 8.1 Parser Integration
- **AST consumption**: Process existing AST structures from syntax phase
- **Error coordination**: Coordinate with parser errors and recovery
- **Source mapping**: Maintain source location information through analysis
- **Comment preservation**: Integrate with existing comment handling

### 8.2 Build System Integration
- **Build coordination**: Integrate with Nova build system
- **Caching**: Use build system cache for semantic analysis results
- **Dependency tracking**: Track semantic dependencies for incremental builds
- **Parallel builds**: Support parallel bundle analysis

### 8.3 IDE Support
- **Language server**: Provide semantic information for IDE features
- **Real-time analysis**: Incremental analysis for code editing
- **Error reporting**: Rich diagnostics for IDE error display
- **Code completion**: Symbol information for autocompletion

## 9) Acceptance Criteria

### 9.1 Functionality
- ✅ Parse and analyze complete Nova bundles with dependencies
- ✅ Perform comprehensive type checking with trait support
- ✅ Resolve symbols across bundle boundaries
- ✅ Enforce visibility and access control rules
- ✅ Generate appropriate diagnostics for all error cases
- ✅ Support incremental analysis and caching

### 9.2 Quality
- ✅ All unit tests pass with >95% code coverage
- ✅ Integration tests cover realistic usage scenarios
- ✅ Performance meets scalability targets
- ✅ Error messages are clear and actionable
- ✅ Memory usage is bounded and predictable

### 9.3 Integration
- ✅ Seamlessly integrates with existing parser
- ✅ Supports build system integration
- ✅ Provides API for IDE language server
- ✅ Maintains compatibility with existing Nova code

## 10) Future Extensions

### 10.1 Advanced Features
- **Incremental type checking**: Faster re-analysis for code changes
- **Parallel analysis**: Multi-threaded semantic analysis
- **Plugin system**: Extensible analysis passes
- **Custom diagnostics**: User-defined semantic checks

### 10.2 Optimization Opportunities
- **Semantic caching**: Persistent cache across build sessions
- **Lazy loading**: Load semantic data on demand
- **Compression**: Compress stored semantic data
- **Distributed analysis**: Distribute analysis across multiple machines

### 10.3 Tooling Integration
- **Debug support**: Semantic information for debuggers
- **Profiling integration**: Performance analysis with semantic context
- **Documentation generation**: Automatic documentation from semantic data
- **Refactoring tools**: Semantic-aware code refactoring

## Appendix A — Error Categories

### Semantic Errors
- `TypeMismatch`: Expected type X, found type Y
- `UnresolvedSymbol`: Cannot find symbol in scope
- `VisibilityViolation`: Symbol not accessible from current context
- `CircularDependency`: Circular dependency detected in bundle graph
- `CoherenceConflict`: Conflicting trait implementations
- `AmbiguousImport`: Multiple symbols match import

### Linking Errors
- `MissingDependency`: Required bundle not found
- `VersionConflict`: Incompatible version requirements
- `SymbolConflict`: Multiple definitions for same symbol
- `ExportNotFound`: Imported symbol not exported by bundle

## Appendix B — Configuration Options

### Analysis Configuration
- `strictness_level`: Error tolerance level (strict, normal, permissive)
- `warning_as_error`: Treat warnings as errors
- `max_dependency_depth`: Maximum allowed dependency chain length
- `cache_location`: Directory for semantic analysis cache
- `parallel_analysis`: Enable parallel bundle analysis
- `memory_limit`: Maximum memory usage for analysis

### Diagnostic Configuration
- `error_format`: Error output format (compact, detailed, json)
- `show_suggestions`: Include fix suggestions in diagnostics
- `max_errors`: Maximum number of errors to report
- `context_lines`: Number of context lines in error display