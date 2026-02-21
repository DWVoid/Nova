//! Bundle manifest parsing and validation
//!
//! This module handles the parsing of bundle configuration files (typically named
//! Nova.toml or similar) that specify bundle metadata, dependencies, and build configuration.

use crate::semantic::bundle::{BundleName, Version, VersionConstraint, DependencyVisibility, BundleMetadata};
use crate::semantic::{SemanticDiagnostic, DiagnosticSeverity, DiagnosticCategory};
use crate::lexical::token::Position;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Bundle manifest configuration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BundleManifest {
    /// Bundle identification
    pub bundle: BundleInfo,
    /// Dependencies on other bundles
    pub dependencies: HashMap<String, DependencySpec>,
    /// Build configuration
    pub build: Option<BuildSection>,
    /// Feature flags
    pub features: Option<HashMap<String, Vec<String>>>,
}

/// Bundle information section
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BundleInfo {
    /// Bundle name
    pub name: BundleName,
    /// Bundle version
    pub version: Version,
    /// Authors
    pub authors: Option<Vec<String>>,
    /// Description
    pub description: Option<String>,
    /// License identifier
    pub license: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// Keywords for discovery
    pub keywords: Option<Vec<String>>,
    /// Categories
    pub categories: Option<Vec<String>>,
}

/// Dependency specification
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DependencySpec {
    /// Version constraint
    pub version: VersionConstraint,
    /// Visibility (public/private)
    pub visibility: DependencyVisibility,
    /// Optional features to enable
    pub features: Vec<String>,
    /// Whether this is an optional dependency
    pub optional: bool,
}

/// Build configuration section
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BuildSection {
    /// Target platform
    pub target: Option<String>,
    /// Optimization level
    pub optimization: Option<String>,
    /// Debug symbols
    pub debug: Option<bool>,
    /// Custom build steps
    pub scripts: Option<Vec<String>>,
}

impl BundleManifest {
    /// Load manifest from a TOML file
    #[allow(dead_code)]
    pub fn load_from_file<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Vec<SemanticDiagnostic>> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            vec![SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Failed to read manifest file: {}", e),
                location: Position::new_start(),
                category: DiagnosticCategory::DependencyError,
            }]
        })?;

        Self::parse_toml(&content)
    }

    /// Parse manifest from TOML content
    pub fn parse_toml(_content: &str) -> Result<Self, Vec<SemanticDiagnostic>> {
        // For now, return a minimal default manifest
        // TODO: Implement actual TOML parsing
        let manifest = BundleManifest {
            bundle: BundleInfo {
                name: BundleName::from("example"),
                version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                    pre_release: None,
                },
                authors: Some(vec!["Anonymous".to_string()]),
                description: Some("Example Nova bundle".to_string()),
                license: Some("MIT".to_string()),
                repository: None,
                keywords: Some(vec!["nova".to_string()]),
                categories: Some(vec!["development".to_string()]),
            },
            dependencies: HashMap::new(),
            build: Some(BuildSection {
                target: Some("native".to_string()),
                optimization: Some("debug".to_string()),
                debug: Some(true),
                scripts: None,
            }),
            features: None,
        };

        Ok(manifest)
    }

    /// Convert manifest to bundle metadata
    #[allow(dead_code)]
    pub fn to_metadata(&self) -> BundleMetadata {
        BundleMetadata {
            authors: self.bundle.authors.clone().unwrap_or_default(),
            description: self.bundle.description.clone(),
            license: self.bundle.license.clone(),
            repository: self.bundle.repository.clone(),
            keywords: self.bundle.keywords.clone().unwrap_or_default(),
            categories: self.bundle.categories.clone().unwrap_or_default(),
            build_config: Default::default(), // TODO: Convert from build section
            feature_flags: HashMap::new(), // TODO: Process features
        }
    }

    /// Validate the manifest for consistency
    #[allow(dead_code)]
    pub fn validate(&self) -> Vec<SemanticDiagnostic> {
        let mut diagnostics = Vec::new();

        // Validate bundle name
        if self.bundle.name.0.is_empty() {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "Bundle name cannot be empty".to_string(),
                location: Position::new_start(),
                category: DiagnosticCategory::DependencyError,
            });
        }

        // Validate version
        if self.bundle.version.major == 0 && 
           self.bundle.version.minor == 0 && 
           self.bundle.version.patch == 0 {
            diagnostics.push(SemanticDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "Version 0.0.0 is not recommended for published bundles".to_string(),
                location: Position::new_start(),
                category: DiagnosticCategory::DependencyError,
            });
        }

        // Check for circular dependencies in features
        if let Some(ref features) = self.features {
            self.check_circular_features(features, &mut diagnostics);
        }

        diagnostics
    }

    /// Check for circular dependencies in feature definitions
    #[allow(dead_code)]
    fn check_circular_features(
        &self,
        features: &HashMap<String, Vec<String>>,
        diagnostics: &mut Vec<SemanticDiagnostic>,
    ) {
        // TODO: Implement circular feature dependency detection
        // For now, just check for self-references
        for (feature_name, dependencies) in features {
            if dependencies.contains(feature_name) {
                diagnostics.push(SemanticDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    message: format!("Feature '{}' cannot depend on itself", feature_name),
                    location: Position::new_start(),
                    category: DiagnosticCategory::DependencyError,
                });
            }
        }
    }
}

/// Parse a version string into a Version struct
#[allow(dead_code)]
pub fn parse_version(version_str: &str) -> Result<Version, SemanticDiagnostic> {
    // First split by dash to separate base version from pre-release
    let (base_version, pre_release) = if let Some(dash_pos) = version_str.find('-') {
        let base = &version_str[..dash_pos];
        let pre = Some(version_str[dash_pos + 1..].to_string());
        (base, pre)
    } else {
        (version_str, None)
    };
    
    let parts: Vec<&str> = base_version.split('.').collect();
    
    if parts.len() < 3 {
        return Err(SemanticDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: format!("Invalid version format: '{}'. Expected major.minor.patch", version_str),
            location: Position::new_start(),
            category: DiagnosticCategory::DependencyError,
        });
    }

    let major = parts[0].parse().map_err(|_| SemanticDiagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!("Invalid major version: '{}'", parts[0]),
        location: Position::new_start(),
        category: DiagnosticCategory::DependencyError,
    })?;

    let minor = parts[1].parse().map_err(|_| SemanticDiagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!("Invalid minor version: '{}'", parts[1]),
        location: Position::new_start(),
        category: DiagnosticCategory::DependencyError,
    })?;

    let patch = parts[2].parse().map_err(|_| SemanticDiagnostic {
        severity: DiagnosticSeverity::Error,
        message: format!("Invalid patch version: '{}'", parts[2]),
        location: Position::new_start(),
        category: DiagnosticCategory::DependencyError,
    })?;

    Ok(Version {
        major,
        minor,
        patch,
        pre_release,
    })
}

/// Parse a version constraint string
#[allow(dead_code)]
pub fn parse_version_constraint(constraint_str: &str) -> Result<VersionConstraint, SemanticDiagnostic> {
    if constraint_str.starts_with('^') {
        // Compatible version constraint (^1.2.3)
        let version = parse_version(&constraint_str[1..])?;
        Ok(VersionConstraint::Compatible(version))
    } else if constraint_str.starts_with('=') {
        // Exact version constraint (=1.2.3)
        let version = parse_version(&constraint_str[1..])?;
        Ok(VersionConstraint::Exact(version))
    } else {
        // Default to exact match
        let version = parse_version(constraint_str)?;
        Ok(VersionConstraint::Exact(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        let version = parse_version("1.2.3").unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
        assert!(version.pre_release.is_none());
    }

    #[test]
    fn test_parse_prerelease_version() {
        let version = parse_version("1.0.0-alpha.1").unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 0);
        assert_eq!(version.patch, 0);
        assert_eq!(version.pre_release, Some("alpha.1".to_string()));
    }

    #[test]
    fn test_parse_version_constraint() {
        let constraint = parse_version_constraint("^1.2.3").unwrap();
        match constraint {
            VersionConstraint::Compatible(v) => {
                assert_eq!(v.major, 1);
                assert_eq!(v.minor, 2);
                assert_eq!(v.patch, 3);
            }
            _ => panic!("Expected compatible constraint"),
        }
    }

    #[test]
    fn test_manifest_validation() {
        let manifest = BundleManifest::parse_toml("").unwrap();
        let diagnostics = manifest.validate();
        
        // Should have at least one diagnostic about version 0.0.0
        assert!(!diagnostics.is_empty());
    }
}