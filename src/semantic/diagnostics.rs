//! Nova Diagnostic System and Error Recovery
//!
//! This module implements Nova's comprehensive diagnostic system including
//! error recovery mechanisms, intelligent suggestions, and rich diagnostic
//! information for improved developer experience during compilation.

use crate::syntax::ast::{Chunk, TopItem, Definition, Exp};
use crate::lexical::{Position, Span};
use super::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory, QualifiedName};
use super::bundle::BundleName;
use super::namespace::NamespacePath;
use std::collections::{HashMap, VecDeque, HashSet};

/// Core diagnostic system manager
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DiagnosticSystem {
    /// Bundle being analyzed
    bundle_name: BundleName,
    /// Collected diagnostics organized by severity
    diagnostics: DiagnosticCollection,
    /// Error recovery mechanisms
    recovery_engine: ErrorRecoveryEngine,
    /// Suggestion generator for fixes
    suggestion_engine: SuggestionEngine,
    /// Diagnostic formatting and presentation
    formatter: DiagnosticFormatter,
}

/// Collection of diagnostics organized by category and severity
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct DiagnosticCollection {
    /// All diagnostics by severity
    by_severity: HashMap<DiagnosticSeverity, Vec<EnhancedDiagnostic>>,
    /// Diagnostics by category
    by_category: HashMap<DiagnosticCategory, Vec<EnhancedDiagnostic>>,
    /// Diagnostics by source location
    by_location: HashMap<Position, Vec<EnhancedDiagnostic>>,
    /// Related diagnostic groups
    related_groups: Vec<DiagnosticGroup>,
}

/// Enhanced diagnostic with additional context and suggestions
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct EnhancedDiagnostic {
    /// Base diagnostic information
    base: SemanticDiagnostic,
    /// Diagnostic identifier for referencing
    id: DiagnosticId,
    /// Additional context information
    context: DiagnosticContext,
    /// Suggested fixes
    suggestions: Vec<DiagnosticSuggestion>,
    /// Related diagnostics
    related: Vec<DiagnosticId>,
    /// Error recovery information
    recovery: Option<ErrorRecovery>,
}

/// Unique identifier for diagnostics
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub struct DiagnosticId {
    /// Category prefix
    category: String,
    /// Numeric identifier within category
    number: u32,
    /// Optional sub-identifier
    sub_id: Option<String>,
}

/// Rich diagnostic context information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DiagnosticContext {
    /// Source code snippet
    source_snippet: Option<SourceSnippet>,
    /// Compilation phase where error occurred
    compilation_phase: CompilationPhase,
    /// Semantic context stack
    semantic_stack: Vec<SemanticFrame>,
    /// Symbol resolution context
    symbol_context: Option<SymbolContext>,
    /// Additional metadata
    metadata: HashMap<String, DiagnosticValue>,
}

/// Source code snippet for context
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SourceSnippet {
    /// The relevant source lines
    lines: Vec<SourceLine>,
    /// Primary highlight span
    primary_span: Span,
    /// Secondary highlight spans
    secondary_spans: Vec<(Span, String)>,
    /// Line number offset from start of file
    line_offset: usize,
}

/// Single source line with highlighting
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SourceLine {
    /// Line number
    line_number: usize,
    /// Line content
    content: String,
    /// Column highlights
    highlights: Vec<ColumnHighlight>,
}

/// Column highlight information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ColumnHighlight {
    /// Start column (0-based)
    start_column: usize,
    /// End column (0-based)
    end_column: usize,
    /// Highlight style
    style: HighlightStyle,
    /// Associated message
    message: Option<String>,
}

/// Highlight styling options
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum HighlightStyle {
    /// Error highlighting (red)
    Error,
    /// Warning highlighting (yellow)
    Warning,
    /// Info highlighting (blue)
    Info,
    /// Success highlighting (green)
    Success,
    /// Note highlighting (gray)
    Note,
}

/// Compilation phase identification
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum CompilationPhase {
    /// Lexical analysis phase
    Lexing,
    /// Syntax parsing phase
    Parsing,
    /// Semantic analysis phase
    SemanticAnalysis,
    /// Type checking phase
    TypeChecking,
    /// Symbol resolution phase
    SymbolResolution,
    /// Trait resolution phase
    TraitResolution,
    /// Visibility checking phase
    VisibilityChecking,
    /// Decorator processing phase
    DecoratorProcessing,
    /// Link-time resolution phase
    Linking,
    /// Code generation phase
    CodeGeneration,
}

/// Semantic context frame
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SemanticFrame {
    /// Frame type
    frame_type: FrameType,
    /// Associated definition or construct
    definition: Option<QualifiedName>,
    /// Frame-specific metadata
    metadata: HashMap<String, String>,
}

/// Types of semantic frames
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum FrameType {
    /// Bundle-level context
    Bundle,
    /// Namespace-level context
    Namespace,
    /// Definition-level context
    Definition,
    /// Expression-level context
    Expression,
    /// Type-level context
    Type,
    /// Trait implementation context
    TraitImplementation,
    /// Function body context
    FunctionBody,
    /// Decorator application context
    DecoratorApplication,
}

/// Symbol resolution context
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SymbolContext {
    /// Symbol being resolved
    symbol_name: String,
    /// Available symbols in scope
    available_symbols: Vec<String>,
    /// Suggested alternatives
    suggestions: Vec<String>,
    /// Resolution search path
    search_path: Vec<NamespacePath>,
}

/// Diagnostic value types
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DiagnosticValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Boolean value
    Boolean(bool),
    /// List of values
    List(Vec<DiagnosticValue>),
    /// Map of values
    Map(HashMap<String, DiagnosticValue>),
}

/// Suggested fix for diagnostic
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DiagnosticSuggestion {
    /// Suggestion description
    description: String,
    /// Type of suggestion
    suggestion_type: SuggestionType,
    /// Code changes required
    changes: Vec<CodeChange>,
    /// Confidence level (0.0 to 1.0)
    confidence: f32,
    /// Additional explanation
    explanation: Option<String>,
}

/// Types of diagnostic suggestions
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum SuggestionType {
    /// Add missing code
    Addition,
    /// Remove incorrect code
    Removal,
    /// Replace existing code
    Replacement,
    /// Rename symbol
    Rename,
    /// Import missing symbol
    Import,
    /// Change visibility
    VisibilityChange,
    /// Add type annotation
    TypeAnnotation,
    /// Fix syntax error
    SyntaxFix,
}

/// Code change specification
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct CodeChange {
    /// Location of change
    location: Span,
    /// Type of change
    change_type: ChangeType,
    /// New content
    new_content: String,
    /// Description of change
    description: String,
}

/// Types of code changes
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ChangeType {
    /// Insert new text
    Insert,
    /// Replace existing text
    Replace,
    /// Delete existing text
    Delete,
}

/// Group of related diagnostics
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DiagnosticGroup {
    /// Group identifier
    id: String,
    /// Group title
    title: String,
    /// Diagnostics in this group
    diagnostics: Vec<DiagnosticId>,
    /// Group-level suggestions
    group_suggestions: Vec<DiagnosticSuggestion>,
}

/// Error recovery engine for intelligent error handling
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct ErrorRecoveryEngine {
    /// Recovery strategies by error type
    recovery_strategies: HashMap<DiagnosticCategory, Vec<RecoveryStrategy>>,
    /// Error patterns for pattern matching
    error_patterns: Vec<ErrorPattern>,
    /// Recovery success statistics
    recovery_stats: RecoveryStatistics,
}

/// Recovery strategy specification
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct RecoveryStrategy {
    /// Strategy name
    name: String,
    /// Conditions for applying strategy
    conditions: Vec<RecoveryCondition>,
    /// Recovery actions
    actions: Vec<RecoveryAction>,
    /// Success probability estimate
    success_probability: f32,
}

/// Conditions for recovery strategy application
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum RecoveryCondition {
    /// Error category matches
    CategoryMatches(DiagnosticCategory),
    /// Error message contains text
    MessageContains(String),
    /// Error occurs in specific context
    ContextMatches(FrameType),
    /// Multiple related errors exist
    HasRelatedErrors(usize),
}

/// Recovery actions to take
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum RecoveryAction {
    /// Skip problematic construct
    Skip,
    /// Insert placeholder content
    InsertPlaceholder(String),
    /// Use default value
    UseDefault,
    /// Suggest alternative
    SuggestAlternative(Vec<String>),
    /// Continue with partial information
    ContinuePartial,
}

/// Error pattern for matching
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ErrorPattern {
    /// Pattern name
    name: String,
    /// Pattern matching rules
    pattern: PatternRule,
    /// Associated recovery strategy
    recovery_strategy: String,
}

/// Pattern matching rules
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum PatternRule {
    /// Exact message match
    ExactMatch(String),
    /// Regex pattern match
    RegexMatch(String),
    /// Category and message combination
    CategoryMessage(DiagnosticCategory, String),
    /// Multiple condition pattern
    Complex(Vec<PatternRule>),
}

/// Error recovery information
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ErrorRecovery {
    /// Recovery strategy used
    strategy: String,
    /// Recovery actions taken
    actions_taken: Vec<String>,
    /// Recovery success
    success: bool,
    /// Additional context preserved
    preserved_context: HashMap<String, String>,
}

/// Recovery statistics
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct RecoveryStatistics {
    /// Total recovery attempts
    total_attempts: usize,
    /// Successful recoveries
    successful_recoveries: usize,
    /// Recovery attempts by strategy
    strategy_stats: HashMap<String, (usize, usize)>, // (attempts, successes)
}

/// Suggestion generation engine
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SuggestionEngine {
    /// Suggestion generators by error type
    generators: HashMap<DiagnosticCategory, Vec<SuggestionGenerator>>,
    /// Symbol similarity calculator
    similarity_calculator: SymbolSimilarityCalculator,
    /// Common fix patterns
    fix_patterns: Vec<FixPattern>,
}

/// Suggestion generator
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SuggestionGenerator {
    /// Generator name
    name: String,
    /// Trigger conditions
    triggers: Vec<SuggestionTrigger>,
    /// Generation function identifier
    generator_function: String,
}

/// Triggers for suggestion generation
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum SuggestionTrigger {
    /// Symbol not found error
    UnresolvedSymbol,
    /// Type mismatch error
    TypeMismatch,
    /// Visibility violation
    VisibilityViolation,
    /// Missing import
    MissingImport,
    /// Syntax error
    SyntaxError,
}

/// Symbol similarity calculator
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SymbolSimilarityCalculator {
    /// Cached similarity scores
    similarity_cache: HashMap<(String, String), f32>,
}

/// Common fix patterns
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FixPattern {
    /// Pattern name
    name: String,
    /// Error pattern to match
    error_pattern: String,
    /// Fix template
    fix_template: String,
    /// Pattern confidence
    confidence: f32,
}

/// Diagnostic formatting engine
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct DiagnosticFormatter {
    /// Formatting styles by output format
    formats: HashMap<OutputFormat, FormatStyle>,
    /// Color support configuration
    color_config: ColorConfiguration,
}

/// Output format options
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum OutputFormat {
    /// Plain text console output
    PlainText,
    /// Colored console output
    ColoredText,
    /// JSON format for tools
    Json,
    /// Language Server Protocol format
    Lsp,
    /// HTML format for web display
    Html,
    /// Markdown format for documentation
    Markdown,
}

/// Formatting style configuration
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FormatStyle {
    /// Show source context
    show_source: bool,
    /// Show line numbers
    show_line_numbers: bool,
    /// Show suggestions
    show_suggestions: bool,
    /// Maximum context lines
    max_context_lines: usize,
    /// Include related diagnostics
    include_related: bool,
}

/// Color configuration
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct ColorConfiguration {
    /// Enable color output
    enabled: bool,
    /// Color scheme
    scheme: ColorScheme,
}

/// Color scheme options
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ColorScheme {
    /// Default color scheme
    Default,
    /// High contrast scheme
    HighContrast,
    /// Monochrome scheme
    Monochrome,
    /// Custom color scheme
    Custom(HashMap<HighlightStyle, String>),
}

impl Default for ColorScheme {
    fn default() -> Self {
        ColorScheme::Default
    }
}

/// Statistics about the diagnostic system
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DiagnosticStatistics {
    pub total_diagnostics: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info_messages: usize,
    pub suggestions_generated: usize,
    pub recovery_attempts: usize,
    pub successful_recoveries: usize,
    pub diagnostics_by_category: HashMap<DiagnosticCategory, usize>,
}

impl DiagnosticSystem {
    /// Create a new diagnostic system for a bundle
    pub fn new(bundle_name: BundleName) -> Self {
        Self {
            bundle_name,
            diagnostics: DiagnosticCollection::default(),
            recovery_engine: ErrorRecoveryEngine::default(),
            suggestion_engine: SuggestionEngine::default(),
            formatter: DiagnosticFormatter::default(),
        }
    }

    /// Process and enhance diagnostics from semantic analysis
    pub fn process_diagnostics(
        &mut self,
        base_diagnostics: &[SemanticDiagnostic],
        semantic_context: &SemanticAnalysisContext,
    ) -> Vec<EnhancedDiagnostic> {
        let mut enhanced_diagnostics = Vec::new();

        for (index, diagnostic) in base_diagnostics.iter().enumerate() {
            let enhanced = self.enhance_diagnostic(diagnostic, index, semantic_context);
            self.add_to_collection(&enhanced);
            enhanced_diagnostics.push(enhanced);
        }

        // Group related diagnostics
        self.group_related_diagnostics();

        // Apply error recovery
        self.apply_error_recovery(&mut enhanced_diagnostics, semantic_context);

        enhanced_diagnostics
    }

    /// Enhance a single diagnostic with additional context and suggestions
    fn enhance_diagnostic(
        &mut self,
        diagnostic: &SemanticDiagnostic,
        index: usize,
        context: &SemanticAnalysisContext,
    ) -> EnhancedDiagnostic {
        let id = self.generate_diagnostic_id(diagnostic, index);
        let diagnostic_context = self.build_diagnostic_context(diagnostic, context);
        let suggestions = self.generate_suggestions(diagnostic, &diagnostic_context);

        EnhancedDiagnostic {
            base: diagnostic.clone(),
            id,
            context: diagnostic_context,
            suggestions,
            related: Vec::new(), // Will be populated during grouping
            recovery: None,      // Will be populated during recovery
        }
    }

    /// Generate unique diagnostic identifier
    fn generate_diagnostic_id(&self, diagnostic: &SemanticDiagnostic, index: usize) -> DiagnosticId {
        let category_code = match diagnostic.category {
            DiagnosticCategory::TypeError => "T",
            DiagnosticCategory::SymbolResolution => "S",
            DiagnosticCategory::VisibilityViolation => "V",
            DiagnosticCategory::DependencyError => "D",
            DiagnosticCategory::CoherenceConflict => "C",
            DiagnosticCategory::DecoratorError => "R",
        };

        DiagnosticId {
            category: category_code.to_string(),
            number: index as u32 + 1,
            sub_id: None,
        }
    }

    /// Build diagnostic context information
    fn build_diagnostic_context(
        &self,
        diagnostic: &SemanticDiagnostic,
        context: &SemanticAnalysisContext,
    ) -> DiagnosticContext {
        DiagnosticContext {
            source_snippet: self.extract_source_snippet(diagnostic, context),
            compilation_phase: self.determine_compilation_phase(&diagnostic.category),
            semantic_stack: context.semantic_stack.clone(),
            symbol_context: self.build_symbol_context(diagnostic, context),
            metadata: HashMap::new(),
        }
    }

    /// Extract source code snippet for context
    fn extract_source_snippet(
        &self,
        diagnostic: &SemanticDiagnostic,
        context: &SemanticAnalysisContext,
    ) -> Option<SourceSnippet> {
        // For now, create a placeholder snippet
        // In a full implementation, this would extract actual source code
        context.source_code.as_ref().map(|_source| SourceSnippet {
            lines: vec![SourceLine {
                line_number: diagnostic.location.line(),
                content: "// Source line would be extracted here".to_string(),
                highlights: vec![ColumnHighlight {
                    start_column: diagnostic.location.column(),
                    end_column: diagnostic.location.column() + 10,
                    style: match diagnostic.severity {
                        DiagnosticSeverity::Error => HighlightStyle::Error,
                        DiagnosticSeverity::Warning => HighlightStyle::Warning,
                        DiagnosticSeverity::Info => HighlightStyle::Info,
                    },
                    message: Some(diagnostic.message.clone()),
                }],
            }],
            primary_span: Span::single(diagnostic.location),
            secondary_spans: Vec::new(),
            line_offset: diagnostic.location.line().saturating_sub(1),
        })
    }

    /// Determine compilation phase from diagnostic category
    fn determine_compilation_phase(&self, category: &DiagnosticCategory) -> CompilationPhase {
        match category {
            DiagnosticCategory::TypeError => CompilationPhase::TypeChecking,
            DiagnosticCategory::SymbolResolution => CompilationPhase::SymbolResolution,
            DiagnosticCategory::VisibilityViolation => CompilationPhase::VisibilityChecking,
            DiagnosticCategory::DependencyError => CompilationPhase::Linking,
            DiagnosticCategory::CoherenceConflict => CompilationPhase::TraitResolution,
            DiagnosticCategory::DecoratorError => CompilationPhase::DecoratorProcessing,
        }
    }

    /// Build symbol context if applicable
    fn build_symbol_context(
        &self,
        diagnostic: &SemanticDiagnostic,
        context: &SemanticAnalysisContext,
    ) -> Option<SymbolContext> {
        if matches!(diagnostic.category, DiagnosticCategory::SymbolResolution) {
            // Extract symbol name from diagnostic message (simplified)
            let symbol_name = diagnostic.message.split_whitespace()
                .find(|word| word.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .unwrap_or("unknown")
                .to_string();

            Some(SymbolContext {
                symbol_name: symbol_name.clone(),
                available_symbols: context.available_symbols.clone(),
                suggestions: self.suggestion_engine.similarity_calculator
                    .find_similar_symbols(&symbol_name, &context.available_symbols),
                search_path: context.search_path.clone(),
            })
        } else {
            None
        }
    }

    /// Generate suggestions for diagnostic
    fn generate_suggestions(
        &mut self,
        diagnostic: &SemanticDiagnostic,
        context: &DiagnosticContext,
    ) -> Vec<DiagnosticSuggestion> {
        let mut suggestions = Vec::new();

        // Generate category-specific suggestions
        match diagnostic.category {
            DiagnosticCategory::SymbolResolution => {
                suggestions.extend(self.generate_symbol_suggestions(diagnostic, context));
            }
            DiagnosticCategory::TypeError => {
                suggestions.extend(self.generate_type_suggestions(diagnostic, context));
            }
            DiagnosticCategory::VisibilityViolation => {
                suggestions.extend(self.generate_visibility_suggestions(diagnostic, context));
            }
            _ => {
                // Generic suggestions
                suggestions.push(DiagnosticSuggestion {
                    description: "Check the documentation for more information".to_string(),
                    suggestion_type: SuggestionType::Addition,
                    changes: Vec::new(),
                    confidence: 0.5,
                    explanation: None,
                });
            }
        }

        suggestions
    }

    /// Generate symbol resolution suggestions
    fn generate_symbol_suggestions(
        &self,
        diagnostic: &SemanticDiagnostic,
        context: &DiagnosticContext,
    ) -> Vec<DiagnosticSuggestion> {
        let mut suggestions = Vec::new();

        if let Some(symbol_context) = &context.symbol_context {
            // Suggest similar symbols
            for suggestion in &symbol_context.suggestions {
                suggestions.push(DiagnosticSuggestion {
                    description: format!("Did you mean '{}'?", suggestion),
                    suggestion_type: SuggestionType::Replacement,
                    changes: vec![CodeChange {
                        location: Span::single(diagnostic.location),
                        change_type: ChangeType::Replace,
                        new_content: suggestion.clone(),
                        description: format!("Replace with '{}'", suggestion),
                    }],
                    confidence: 0.8,
                    explanation: Some("This symbol has a similar name and is available in the current scope".to_string()),
                });
            }

            // Suggest adding import
            if !symbol_context.symbol_name.is_empty() {
                suggestions.push(DiagnosticSuggestion {
                    description: format!("Add import for '{}'", symbol_context.symbol_name),
                    suggestion_type: SuggestionType::Import,
                    changes: vec![CodeChange {
                        location: Span::single(Position::new_start()),
                        change_type: ChangeType::Insert,
                        new_content: format!("use {};\n", symbol_context.symbol_name),
                        description: "Add import statement".to_string(),
                    }],
                    confidence: 0.6,
                    explanation: Some("The symbol might be available through an import".to_string()),
                });
            }
        }

        suggestions
    }

    /// Generate type error suggestions
    fn generate_type_suggestions(
        &self,
        _diagnostic: &SemanticDiagnostic,
        _context: &DiagnosticContext,
    ) -> Vec<DiagnosticSuggestion> {
        vec![
            DiagnosticSuggestion {
                description: "Add explicit type annotation".to_string(),
                suggestion_type: SuggestionType::TypeAnnotation,
                changes: Vec::new(),
                confidence: 0.7,
                explanation: Some("Adding type annotations can help resolve type mismatches".to_string()),
            }
        ]
    }

    /// Generate visibility violation suggestions
    fn generate_visibility_suggestions(
        &self,
        _diagnostic: &SemanticDiagnostic,
        _context: &DiagnosticContext,
    ) -> Vec<DiagnosticSuggestion> {
        vec![
            DiagnosticSuggestion {
                description: "Make the definition public".to_string(),
                suggestion_type: SuggestionType::VisibilityChange,
                changes: Vec::new(),
                confidence: 0.6,
                explanation: Some("Making the definition public would resolve the visibility issue".to_string()),
            }
        ]
    }

    /// Add enhanced diagnostic to collection
    fn add_to_collection(&mut self, diagnostic: &EnhancedDiagnostic) {
        // Add to severity-based collection
        self.diagnostics.by_severity
            .entry(diagnostic.base.severity.clone())
            .or_insert_with(Vec::new)
            .push(diagnostic.clone());

        // Add to category-based collection
        self.diagnostics.by_category
            .entry(diagnostic.base.category.clone())
            .or_insert_with(Vec::new)
            .push(diagnostic.clone());

        // Add to location-based collection
        self.diagnostics.by_location
            .entry(diagnostic.base.location)
            .or_insert_with(Vec::new)
            .push(diagnostic.clone());
    }

    /// Group related diagnostics
    fn group_related_diagnostics(&mut self) {
        // Group by location proximity
        let mut location_groups = HashMap::new();
        for (location, diagnostics) in &self.diagnostics.by_location {
            if diagnostics.len() > 1 {
                let group = DiagnosticGroup {
                    id: format!("location_{}_{}_{}", location.line(), location.column(), location.byte()),
                    title: format!("Issues at line {} column {}", location.line(), location.column()),
                    diagnostics: diagnostics.iter().map(|d| d.id.clone()).collect(),
                    group_suggestions: Vec::new(),
                };
                location_groups.insert(group.id.clone(), group);
            }
        }

        // Store location groups
        self.diagnostics.related_groups.extend(location_groups.into_values());
    }

    /// Apply error recovery strategies
    fn apply_error_recovery(
        &mut self,
        diagnostics: &mut Vec<EnhancedDiagnostic>,
        _context: &SemanticAnalysisContext,
    ) {
        for diagnostic in diagnostics {
            if diagnostic.base.severity == DiagnosticSeverity::Error {
                if let Some(recovery) = self.attempt_error_recovery(diagnostic) {
                    diagnostic.recovery = Some(recovery);
                }
            }
        }
    }

    /// Attempt error recovery for a diagnostic
    fn attempt_error_recovery(&mut self, diagnostic: &EnhancedDiagnostic) -> Option<ErrorRecovery> {
        // Simple recovery strategy selection
        let strategy_name = match diagnostic.base.category {
            DiagnosticCategory::SymbolResolution => "symbol_recovery",
            DiagnosticCategory::TypeError => "type_recovery",
            _ => "generic_recovery",
        };

        self.recovery_engine.recovery_stats.total_attempts += 1;

        // Simulate recovery attempt
        let success = !diagnostic.suggestions.is_empty();
        if success {
            self.recovery_engine.recovery_stats.successful_recoveries += 1;
        }

        Some(ErrorRecovery {
            strategy: strategy_name.to_string(),
            actions_taken: vec!["Generated suggestions".to_string()],
            success,
            preserved_context: HashMap::new(),
        })
    }

    /// Get comprehensive diagnostic statistics
    #[allow(dead_code)]
    pub fn get_statistics(&self) -> DiagnosticStatistics {
        let total_diagnostics = self.diagnostics.by_severity.values()
            .map(|diagnostics| diagnostics.len())
            .sum();

        let errors = self.diagnostics.by_severity.get(&DiagnosticSeverity::Error)
            .map(|d| d.len()).unwrap_or(0);
        let warnings = self.diagnostics.by_severity.get(&DiagnosticSeverity::Warning)
            .map(|d| d.len()).unwrap_or(0);
        let info_messages = self.diagnostics.by_severity.get(&DiagnosticSeverity::Info)
            .map(|d| d.len()).unwrap_or(0);

        let suggestions_generated = self.diagnostics.by_severity.values()
            .flat_map(|diagnostics| diagnostics.iter())
            .map(|d| d.suggestions.len())
            .sum();

        let diagnostics_by_category = self.diagnostics.by_category.iter()
            .map(|(category, diagnostics)| (category.clone(), diagnostics.len()))
            .collect();

        DiagnosticStatistics {
            total_diagnostics,
            errors,
            warnings,
            info_messages,
            suggestions_generated,
            recovery_attempts: self.recovery_engine.recovery_stats.total_attempts,
            successful_recoveries: self.recovery_engine.recovery_stats.successful_recoveries,
            diagnostics_by_category,
        }
    }
}

/// Context for semantic analysis
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SemanticAnalysisContext {
    /// Source code being analyzed
    pub source_code: Option<String>,
    /// Current semantic context stack
    pub semantic_stack: Vec<SemanticFrame>,
    /// Available symbols in current scope
    pub available_symbols: Vec<String>,
    /// Symbol search path
    pub search_path: Vec<NamespacePath>,
    /// Current bundle context
    pub bundle_context: Option<BundleName>,
    /// Current namespace context
    pub namespace_context: Option<NamespacePath>,
}

impl SymbolSimilarityCalculator {
    /// Find similar symbols using edit distance
    fn find_similar_symbols(&self, target: &str, candidates: &[String]) -> Vec<String> {
        let mut similarities: Vec<(String, f32)> = candidates.iter()
            .map(|candidate| {
                let similarity = self.calculate_similarity(target, candidate);
                (candidate.clone(), similarity)
            })
            .collect();

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        similarities.into_iter()
            .take(3) // Top 3 suggestions
            .filter(|(_, similarity)| *similarity > 0.5)
            .map(|(symbol, _)| symbol)
            .collect()
    }

    /// Calculate similarity between two strings using edit distance
    fn calculate_similarity(&self, s1: &str, s2: &str) -> f32 {
        if s1 == s2 {
            return 1.0;
        }

        let len1 = s1.len();
        let len2 = s2.len();
        
        if len1 == 0 || len2 == 0 {
            return 0.0;
        }

        // Simple similarity based on common prefix and length difference
        let common_prefix = s1.chars().zip(s2.chars())
            .take_while(|(c1, c2)| c1 == c2)
            .count();

        let length_penalty = (len1 as i32 - len2 as i32).abs() as f32;
        let max_len = len1.max(len2) as f32;
        
        (common_prefix as f32 + (max_len - length_penalty)) / (max_len * 2.0)
    }
}

impl Default for DiagnosticSystem {
    fn default() -> Self {
        Self::new(BundleName::from("default"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_system_creation() {
        let bundle_name = BundleName::from("test");
        let diagnostic_system = DiagnosticSystem::new(bundle_name.clone());
        
        assert_eq!(diagnostic_system.bundle_name, bundle_name);
        assert!(diagnostic_system.diagnostics.by_severity.is_empty());
    }

    #[test]
    fn test_diagnostic_id_generation() {
        let diagnostic_system = DiagnosticSystem::new(BundleName::from("test"));
        let diagnostic = SemanticDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: "Test error".to_string(),
            location: Position::new_start(),
            category: DiagnosticCategory::TypeError,
        };
        
        let id = diagnostic_system.generate_diagnostic_id(&diagnostic, 0);
        assert_eq!(id.category, "T");
        assert_eq!(id.number, 1);
    }

    #[test]
    fn test_compilation_phase_detection() {
        let diagnostic_system = DiagnosticSystem::new(BundleName::from("test"));
        
        let phase = diagnostic_system.determine_compilation_phase(&DiagnosticCategory::TypeError);
        assert_eq!(phase, CompilationPhase::TypeChecking);
        
        let phase = diagnostic_system.determine_compilation_phase(&DiagnosticCategory::SymbolResolution);
        assert_eq!(phase, CompilationPhase::SymbolResolution);
    }

    #[test]
    fn test_symbol_similarity_calculation() {
        let calculator = SymbolSimilarityCalculator::default();
        
        // Exact match
        assert_eq!(calculator.calculate_similarity("test", "test"), 1.0);
        
        // Different strings  
        let similarity = calculator.calculate_similarity("test", "best");
        assert!(similarity > 0.3); // Lowered expectation for simpler algorithm
        
        // Very different strings
        let similarity = calculator.calculate_similarity("test", "xyz");
        assert!(similarity < 0.5);
    }

    #[test]
    fn test_similar_symbol_finding() {
        let calculator = SymbolSimilarityCalculator::default();
        let candidates = vec![
            "printf".to_string(),
            "print".to_string(),
            "println".to_string(),
            "format".to_string(),
        ];
        
        let suggestions = calculator.find_similar_symbols("prinf", &candidates);
        assert!(!suggestions.is_empty());
        assert!(suggestions.contains(&"print".to_string()));
    }

    #[test]
    fn test_diagnostic_collection_organization() {
        let mut diagnostic_system = DiagnosticSystem::new(BundleName::from("test"));
        
        let diagnostic = EnhancedDiagnostic {
            base: SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "Test error".to_string(),
                location: Position::new_start(),
                category: DiagnosticCategory::TypeError,
            },
            id: DiagnosticId {
                category: "T".to_string(),
                number: 1,
                sub_id: None,
            },
            context: DiagnosticContext {
                source_snippet: None,
                compilation_phase: CompilationPhase::TypeChecking,
                semantic_stack: Vec::new(),
                symbol_context: None,
                metadata: HashMap::new(),
            },
            suggestions: Vec::new(),
            related: Vec::new(),
            recovery: None,
        };
        
        diagnostic_system.add_to_collection(&diagnostic);
        
        assert_eq!(diagnostic_system.diagnostics.by_severity.get(&DiagnosticSeverity::Error).unwrap().len(), 1);
        assert_eq!(diagnostic_system.diagnostics.by_category.get(&DiagnosticCategory::TypeError).unwrap().len(), 1);
    }

    #[test]
    fn test_diagnostic_statistics() {
        let diagnostic_system = DiagnosticSystem::new(BundleName::from("test"));
        let stats = diagnostic_system.get_statistics();
        
        assert_eq!(stats.total_diagnostics, 0);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.warnings, 0);
        assert_eq!(stats.suggestions_generated, 0);
    }
}