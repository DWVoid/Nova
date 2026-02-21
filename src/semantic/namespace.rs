//! Namespace resolution and scope management
//!
//! This module implements namespace hierarchy construction, use declaration processing,
//! import resolution, and symbol visibility scoping within namespaces.

use crate::syntax::ast::{Chunk, TopItem, UseDecl, UseTail};
#[cfg(test)]
use crate::syntax::ast::NamespaceDecl;
use crate::lexical::{Position, Span};
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, bundle::BundleName};
use std::collections::HashMap;

/// Hierarchical namespace tree for a bundle
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NamespaceTree {
    /// Root bundle name
    pub root: BundleName,
    /// All namespaces in the bundle
    pub namespaces: HashMap<NamespacePath, NamespaceScope>,
}

/// Path to a namespace within a bundle
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct NamespacePath(pub Vec<String>);

impl NamespacePath {
    /// Create a new namespace path
    pub fn new(parts: Vec<String>) -> Self {
        NamespacePath(parts)
    }

    /// Create root namespace path
    pub fn root() -> Self {
        NamespacePath(Vec::new())
    }

    /// Check if this is the root namespace
    #[allow(dead_code)]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Get parent namespace path
    #[allow(dead_code)]
    pub fn parent(&self) -> Option<NamespacePath> {
        if self.0.len() <= 1 {
            None
        } else {
            Some(NamespacePath(self.0[..self.0.len() - 1].to_vec()))
        }
    }

    /// Create child namespace path
    #[allow(dead_code)]
    pub fn child(&self, name: &str) -> NamespacePath {
        let mut parts = self.0.clone();
        parts.push(name.to_string());
        NamespacePath(parts)
    }

    /// Join path segments with separator
    pub fn join(&self, separator: &str) -> String {
        self.0.join(separator)
    }
}

impl std::fmt::Display for NamespacePath {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.join("::"))
    }
}

/// Scope containing definitions and imports for a namespace
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NamespaceScope {
    /// Local definitions in this namespace
    pub definitions: HashMap<String, LocalDefinition>,
    /// Implementation blocks in this namespace
    pub implementations: Vec<LocalImplementation>,
    /// Nested child namespaces
    pub nested_namespaces: HashMap<String, NamespacePath>,
    /// Resolved imports available in this namespace
    pub imports: Vec<ResolvedImport>,
    /// Source span for this namespace
    pub span: Option<Span>,
}

impl Default for NamespaceScope {
    fn default() -> Self {
        Self {
            definitions: HashMap::new(),
            implementations: Vec::new(),
            nested_namespaces: HashMap::new(),
            imports: Vec::new(),
            span: None,
        }
    }
}

/// Local definition within a namespace
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LocalDefinition {
    /// Reference to the top-level item
    pub item: TopItem,
    /// Whether this definition is exported
    pub is_exported: bool,
    /// Visibility restrictions if any
    pub visibility_scopes: Option<Vec<BundleName>>,
    /// Source location
    pub span: Span,
}

/// Local implementation block within a namespace
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LocalImplementation {
    /// Reference to the implementation
    pub implementation: crate::syntax::ast::Implementation,
    /// Source location
    pub span: Span,
}

/// Resolved import in a namespace
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ResolvedImport {
    /// Source bundle providing the import
    pub source_bundle: BundleName,
    /// Source namespace path
    pub source_namespace: NamespacePath,
    /// Imported items with their local names
    pub imported_items: Vec<ImportedItem>,
    /// When this import is resolved
    pub resolution_time: ImportTime,
    /// Source location of the use declaration
    pub span: Span,
}

/// Individual imported item
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ImportedItem {
    /// Original name in source namespace
    pub original_name: String,
    /// Local name in importing namespace
    pub local_name: String,
    /// Reference to the definition
    pub definition_ref: Option<DefinitionReference>,
}

/// When an import is resolved
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ImportTime {
    /// Resolved during compilation
    CompileTime,
    /// Resolved during bundle linking
    LinkTime,
}

/// Reference to a definition
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DefinitionReference {
    /// Bundle containing the definition
    pub bundle: BundleName,
    /// Namespace path within bundle
    pub namespace: NamespacePath,
    /// Local name of the definition
    pub name: String,
    /// Kind of definition
    pub definition_kind: DefinitionKind,
}

/// Kind of definition being referenced
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum DefinitionKind {
    Type,
    Function,
    Value,
    Trait,
    Implementation,
}

impl NamespaceTree {
    /// Create a new namespace tree for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        let mut namespaces = HashMap::new();
        // Always have a root namespace
        namespaces.insert(NamespacePath::root(), NamespaceScope::default());

        Self {
            root: bundle_name,
            namespaces,
        }
    }

    /// Build namespace tree from compilation units
    pub fn from_compilation_units(
        bundle_name: BundleName,
        chunks: &[Chunk],
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) -> Self {
        let mut tree = Self::new(bundle_name);

        // Process each compilation unit
        for chunk in chunks {
            tree.process_compilation_unit(chunk, diagnostics);
        }

        tree
    }

    /// Process a single compilation unit
    fn process_compilation_unit(
        &mut self,
        chunk: &Chunk,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // Extract namespace path from declaration
        let namespace_path = NamespacePath::new(
            chunk.namespace.path.iter().map(|n| n.value.clone()).collect()
        );

        // Ensure namespace exists
        self.ensure_namespace_exists(&namespace_path, chunk.namespace.span);

        // Process use declarations
        for use_decl in &chunk.uses {
            match self.process_use_declaration(use_decl, &namespace_path) {
                Ok(import) => {
                    if let Some(scope) = self.namespaces.get_mut(&namespace_path) {
                        scope.imports.push(import);
                    }
                }
                Err(err) => diagnostics.push(err),
            }
        }

        // Process top-level items
        for item in &chunk.items {
            self.process_top_level_item(item, &namespace_path, diagnostics);
        }
    }

    /// Ensure a namespace path exists in the tree
    fn ensure_namespace_exists(&mut self, path: &NamespacePath, span: Span) {
        if self.namespaces.contains_key(path) {
            return;
        }

        // Create parent namespaces first
        if let Some(parent_path) = path.parent() {
            self.ensure_namespace_exists(&parent_path, span);

            // Add this namespace to parent's nested list
            if let Some(parent_scope) = self.namespaces.get_mut(&parent_path) {
                if let Some(last_part) = path.0.last() {
                    parent_scope.nested_namespaces.insert(last_part.clone(), path.clone());
                }
            }
        }

        // Create the namespace scope
        let mut scope = NamespaceScope::default();
        scope.span = Some(span);
        self.namespaces.insert(path.clone(), scope);
    }

    /// Process a use declaration
    fn process_use_declaration(
        &self,
        use_decl: &UseDecl,
        _current_namespace: &NamespacePath,
    ) -> Result<ResolvedImport, SemanticDiagnostic> {
        // For now, assume first part of path is bundle name
        if use_decl.path.is_empty() {
            return Err(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "Empty use declaration".to_string(),
                location: use_decl.span.start,
                category: DiagnosticCategory::SymbolResolution,
            });
        }

        let source_bundle = BundleName::from(use_decl.path[0].value.as_str());
        let source_namespace = if use_decl.path.len() > 1 {
            NamespacePath::new(use_decl.path[1..].iter().map(|n| n.value.clone()).collect())
        } else {
            NamespacePath::root()
        };

        let imported_items = match &use_decl.tail {
            Some(UseTail::Selector(items)) => {
                items.iter().map(|item| ImportedItem {
                    original_name: item.name.value.clone(),
                    local_name: item.alias.as_ref()
                        .map(|a| a.value.clone())
                        .unwrap_or_else(|| item.name.value.clone()),
                    definition_ref: None, // Will be resolved later
                }).collect()
            }
            Some(UseTail::Alias(alias)) => {
                // Import the last part of the path with an alias
                if let Some(last_name) = use_decl.path.last() {
                    vec![ImportedItem {
                        original_name: last_name.value.clone(),
                        local_name: alias.value.clone(),
                        definition_ref: None,
                    }]
                } else {
                    Vec::new()
                }
            }
            None => {
                // Import the last part of the path
                if let Some(last_name) = use_decl.path.last() {
                    vec![ImportedItem {
                        original_name: last_name.value.clone(),
                        local_name: last_name.value.clone(),
                        definition_ref: None,
                    }]
                } else {
                    Vec::new()
                }
            }
        };

        Ok(ResolvedImport {
            source_bundle: source_bundle.clone(),
            source_namespace,
            imported_items,
            resolution_time: if source_bundle.to_string() == self.root.to_string() {
                ImportTime::CompileTime
            } else {
                ImportTime::LinkTime
            },
            span: use_decl.span,
        })
    }

    /// Process a top-level item
    fn process_top_level_item(
        &mut self,
        item: &TopItem,
        namespace_path: &NamespacePath,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        match item {
            TopItem::Definition(def) => {
                // Check for duplicate definitions
                if let Some(scope) = self.namespaces.get(&namespace_path) {
                    if scope.definitions.contains_key(&def.name.value) {
                        diagnostics.push(SemanticDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            message: format!(
                                "Duplicate definition of '{}' in namespace '{}'",
                                def.name.value,
                                namespace_path
                            ),
                            location: def.name.span.start,
                            category: DiagnosticCategory::SymbolResolution,
                        });
                        return;
                    }
                }

                // Add definition to namespace
                let local_def = LocalDefinition {
                    item: item.clone(),
                    is_exported: def.visibility.is_some(),
                    visibility_scopes: None, // TODO: Extract from visibility
                    span: def.span,
                };

                if let Some(scope) = self.namespaces.get_mut(namespace_path) {
                    scope.definitions.insert(def.name.value.clone(), local_def);
                }
            }
            TopItem::Implementation(imp) => {
                let local_impl = LocalImplementation {
                    implementation: imp.clone(),
                    span: imp.span,
                };

                if let Some(scope) = self.namespaces.get_mut(namespace_path) {
                    scope.implementations.push(local_impl);
                }
            }
        }
    }

    /// Resolve symbol in namespace, checking imports and parent scopes
    #[allow(dead_code)]
    pub fn resolve_symbol(
        &self,
        symbol_name: &str,
        namespace_path: &NamespacePath,
    ) -> Option<DefinitionReference> {
        // Check local definitions first
        if let Some(scope) = self.namespaces.get(namespace_path) {
            if let Some(local_def) = scope.definitions.get(symbol_name) {
                return Some(DefinitionReference {
                    bundle: self.root.clone(),
                    namespace: namespace_path.clone(),
                    name: symbol_name.to_string(),
                    definition_kind: self.definition_kind_from_item(&local_def.item),
                });
            }

            // Check imports
            for import in &scope.imports {
                for item in &import.imported_items {
                    if item.local_name == symbol_name {
                        if let Some(def_ref) = &item.definition_ref {
                            return Some(def_ref.clone());
                        }
                        // TODO: If not resolved yet, try to resolve from source
                    }
                }
            }
        }

        // Check parent namespaces
        if let Some(parent_path) = namespace_path.parent() {
            return self.resolve_symbol(symbol_name, &parent_path);
        }

        None
    }

    /// Get definition kind from top-level item
    #[allow(dead_code)]
    fn definition_kind_from_item(&self, item: &TopItem) -> DefinitionKind {
        match item {
            TopItem::Definition(def) => {
                match &def.expr {
                    crate::syntax::ast::DefExpr::Struct(_) => DefinitionKind::Type,
                    crate::syntax::ast::DefExpr::Enum(_) => DefinitionKind::Type,
                    crate::syntax::ast::DefExpr::Variant(_) => DefinitionKind::Type,
                    crate::syntax::ast::DefExpr::Trait(_) => DefinitionKind::Trait,
                    crate::syntax::ast::DefExpr::Exp(exp) => {
                        match &exp.kind {
                            crate::syntax::ast::ExpKind::Lambda(_) => DefinitionKind::Function,
                            _ => DefinitionKind::Value,
                        }
                    }
                }
            }
            TopItem::Implementation(_) => DefinitionKind::Implementation,
        }
    }

    /// Get all exported definitions from a namespace
    #[allow(dead_code)]
    pub fn get_exported_definitions(
        &self,
        namespace_path: &NamespacePath,
    ) -> Vec<(String, &LocalDefinition)> {
        if let Some(scope) = self.namespaces.get(namespace_path) {
            scope.definitions
                .iter()
                .filter(|(_, def)| def.is_exported)
                .map(|(name, def)| (name.clone(), def))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all namespaces that have exported definitions
    #[allow(dead_code)]
    pub fn get_public_namespaces(&self) -> Vec<&NamespacePath> {
        self.namespaces
            .iter()
            .filter(|(_, scope)| {
                scope.definitions.values().any(|def| def.is_exported)
            })
            .map(|(path, _)| path)
            .collect()
    }

    /// Validate namespace tree for consistency
    #[allow(dead_code)]
    pub fn validate(&self, diagnostics: &mut Vec<SemanticDiagnostic>) {
        for (path, scope) in &self.namespaces {
            // Check for circular imports
            self.check_circular_imports(path, scope, diagnostics);

            // Check for unresolved imports
            self.check_unresolved_imports(path, scope, diagnostics);

            // Validate nested namespace consistency
            self.validate_nested_namespaces(path, scope, diagnostics);
        }
    }

    /// Check for circular imports in a namespace
    #[allow(dead_code)]
    fn check_circular_imports(
        &self,
        _path: &NamespacePath,
        _scope: &NamespaceScope,
        _diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement circular import detection
        // This requires tracking import chains and detecting cycles
    }

    /// Check for unresolved imports in a namespace
    #[allow(dead_code)]
    fn check_unresolved_imports(
        &self,
        _path: &NamespacePath,
        scope: &NamespaceScope,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for import in &scope.imports {
            for item in &import.imported_items {
                if item.definition_ref.is_none() && import.resolution_time == ImportTime::CompileTime {
                    diagnostics.push(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!(
                            "Unresolved import: '{}' from '{}'",
                            item.original_name,
                            import.source_bundle
                        ),
                        location: import.span.start,
                        category: DiagnosticCategory::SymbolResolution,
                    });
                }
            }
        }
    }

    /// Validate nested namespace consistency
    #[allow(dead_code)]
    fn validate_nested_namespaces(
        &self,
        path: &NamespacePath,
        scope: &NamespaceScope,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        for (child_name, child_path) in &scope.nested_namespaces {
            // Check that child namespace actually exists
            if !self.namespaces.contains_key(child_path) {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!(
                        "Referenced nested namespace '{}' does not exist in '{}'",
                        child_name,
                        path
                    ),
                    location: scope.span.unwrap_or(Span::single(Position::new_start())).start,
                    category: DiagnosticCategory::SymbolResolution,
                });
            }

            // Check that child path is actually a child of current path
            if let Some(parent_path) = child_path.parent() {
                if parent_path != *path {
                    diagnostics.push(SemanticDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "Inconsistent nested namespace: '{}' claims parent '{}' but should be '{}'",
                            child_path,
                            parent_path,
                            path
                        ),
                        location: scope.span.unwrap_or(Span::single(Position::new_start())).start,
                        category: DiagnosticCategory::SymbolResolution,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::ast::Name;

    fn create_test_namespace_decl(path: &[&str], span: Span) -> NamespaceDecl {
        NamespaceDecl {
            span,
            path: path.iter().map(|s| Name {
                value: s.to_string(),
                span,
            }).collect(),
        }
    }

    fn create_test_chunk(namespace_path: &[&str]) -> Chunk {
        let span = Span::single(Position::new_start());
        Chunk {
            span,
            comments: Vec::new(),
            uses: Vec::new(),
            namespace: create_test_namespace_decl(namespace_path, span),
            items: Vec::new(),
        }
    }

    #[test]
    fn test_namespace_path() {
        let root = NamespacePath::root();
        assert!(root.is_root());
        assert_eq!(root.to_string(), "");

        let child = root.child("System");
        assert!(!child.is_root());
        assert_eq!(child.to_string(), "System");

        let grandchild = child.child("Collections");
        assert_eq!(grandchild.to_string(), "System::Collections");
        assert_eq!(grandchild.parent(), Some(child));
    }

    #[test]
    fn test_namespace_tree_creation() {
        let bundle_name = BundleName::from("test");
        let tree = NamespaceTree::new(bundle_name.clone());

        assert_eq!(tree.root, bundle_name);
        assert!(tree.namespaces.contains_key(&NamespacePath::root()));
    }

    #[test]
    fn test_ensure_namespace_exists() {
        let bundle_name = BundleName::from("test");
        let mut tree = NamespaceTree::new(bundle_name);

        let path = NamespacePath::new(vec!["System".to_string(), "Collections".to_string()]);
        let span = Span::single(Position::new_start());
        
        tree.ensure_namespace_exists(&path, span);

        assert!(tree.namespaces.contains_key(&path));
        assert!(tree.namespaces.contains_key(&NamespacePath::new(vec!["System".to_string()])));
        
        // Check parent relationship
        let system_scope = tree.namespaces.get(&NamespacePath::new(vec!["System".to_string()])).unwrap();
        assert!(system_scope.nested_namespaces.contains_key("Collections"));
    }

    #[test]
    fn test_compilation_unit_processing() {
        let bundle_name = BundleName::from("test");
        let chunks = vec![
            create_test_chunk(&["System"]),
            create_test_chunk(&["System", "Collections"]),
        ];

        let mut diagnostics = Vec::new();
        let tree = NamespaceTree::from_compilation_units(bundle_name, &chunks, &mut diagnostics);

        assert!(tree.namespaces.contains_key(&NamespacePath::new(vec!["System".to_string()])));
        assert!(tree.namespaces.contains_key(&NamespacePath::new(vec!["System".to_string(), "Collections".to_string()])));
        assert!(diagnostics.is_empty());
    }
}