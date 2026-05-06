//! # Semantic Analysis Pipeline
//!
//! This module implements the incremental Nova semantic pipeline,
//! built on top of [`nova_incremental`].
//!
//! ## Pipeline
//!
//! ```text
//! ProjectDescriptor  ──[expand]──► Collection<FileStat>
//!                                      └─[load]  → Collection<FileContent>
//!                                                    └─[lex]   → Collection<LexOutput>
//!                                                                  └─[parse] → Collection<ParseOutput>
//!                                                                                 ├─[collect] → String
//!                                                                                 └─[symbol] → Collection<BundleExports>
//!                                                                                                └─[symbol_collect] → Vec<BundleExports>
//! ```

pub mod file_access;
pub mod file_content;
pub mod file_stat;
pub mod project_descriptor;
pub mod load_transform;
pub mod lex_transform;
pub mod parse_transform;
pub mod session;
pub mod symbol_model;

// Top-level re-exports.
pub use file_stat::FileStat;
pub use file_content::FileContent;
pub use file_access::{FileAccess, FileAccessError, MockFileAccess};
pub use project_descriptor::ProjectDescriptor;
pub use load_transform::{
    LexOutput, ParseOutput,
    EXPAND_KEY, LOAD_KEY, LEX_KEY, PARSE_KEY, COLLECT_KEY,
    SYMBOL_KEY, SYMBOL_COLLECT_KEY,
    BUNDLE_FRAGMENT_KEY, BUNDLE_ASSEMBLE_KEY,
    ExpandTransform, LoadTransform, LexTransform, ParseTransform, CollectTransform,
    SymbolTransform, SymbolCollectTransform,
    BundleFragmentTransform, BundleAssembleTransform,
    FileStatByPath, FileContentByPath, LexOutputByPath, ParseOutputByPath,
    SymbolOutputByPath, BundleFragmentByPath,
};
pub use session::{
    SemanticSession,
    project_input_id, bundle_output_id, symbol_output_id, bundle_intermediate_output_id,
    expand_transform_id, load_transform_id, lex_transform_id,
    parse_transform_id, collect_transform_id,
    symbol_transform_id, symbol_collect_transform_id,
    bundle_fragment_transform_id, bundle_assemble_transform_id,
};
pub use symbol_model::{
    BundleExports, ImportedName, ExportedDef, ExportedField,
    ExportedParam, ExportedFunctionSig, ExportedEnumMember,
    ExportedVariantCase, ExportedTraitSig,
    extract as extract_symbols,
};