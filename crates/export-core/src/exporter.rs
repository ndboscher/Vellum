use std::fmt;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use vellum_schema::Document;

/// Target-specific options, keyed by camelCase name (e.g. `{"inlineCss": true}`).
pub type ExportOptions = Map<String, Value>;

/// One generated file, with a path relative to the export root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFile {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub target: String,
    pub files: Vec<OutputFile>,
}

impl ExportResult {
    pub fn file(&self, path: &str) -> Option<&OutputFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    UnknownTarget { id: String, available: Vec<String> },
    DuplicateTarget(String),
    InvalidOptions { target: String, message: String },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::UnknownTarget { id, available } => {
                write!(
                    f,
                    "unknown export target \"{id}\"; available: {}",
                    available.join(", ")
                )
            }
            ExportError::DuplicateTarget(id) => {
                write!(f, "an exporter with id \"{id}\" is already registered")
            }
            ExportError::InvalidOptions { target, message } => {
                write!(f, "invalid {target} options: {message}")
            }
        }
    }
}

impl std::error::Error for ExportError {}

/// A code-generation target. Add a new language by implementing this and
/// registering it; nothing else in the pipeline needs to change.
pub trait Exporter: Send + Sync {
    /// Stable identifier, e.g. `"html"` or `"react"`.
    fn id(&self) -> &'static str;
    /// Name shown in the editor's export menu.
    fn label(&self) -> &'static str;
    fn export(&self, doc: &Document, options: &ExportOptions) -> Result<ExportResult, ExportError>;
}

/// Parses a target's typed options struct from the generic options map.
pub fn parse_options<T: DeserializeOwned>(
    target: &str,
    options: &ExportOptions,
) -> Result<T, ExportError> {
    serde_json::from_value(Value::Object(options.clone())).map_err(|e| {
        ExportError::InvalidOptions {
            target: target.to_owned(),
            message: e.to_string(),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Default)]
pub struct ExporterRegistry {
    exporters: Vec<Box<dyn Exporter>>,
}

impl ExporterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        exporter: impl Exporter + 'static,
    ) -> Result<&mut Self, ExportError> {
        if self.get(exporter.id()).is_some() {
            return Err(ExportError::DuplicateTarget(exporter.id().to_owned()));
        }
        self.exporters.push(Box::new(exporter));
        Ok(self)
    }

    pub fn get(&self, id: &str) -> Option<&dyn Exporter> {
        self.exporters
            .iter()
            .find(|e| e.id() == id)
            .map(|e| e.as_ref())
    }

    /// Registered targets, in registration order.
    pub fn list(&self) -> Vec<TargetInfo> {
        self.exporters
            .iter()
            .map(|e| TargetInfo {
                id: e.id(),
                label: e.label(),
            })
            .collect()
    }

    pub fn export(
        &self,
        id: &str,
        doc: &Document,
        options: &ExportOptions,
    ) -> Result<ExportResult, ExportError> {
        let exporter = self.get(id).ok_or_else(|| ExportError::UnknownTarget {
            id: id.to_owned(),
            available: self.exporters.iter().map(|e| e.id().to_owned()).collect(),
        })?;
        exporter.export(doc, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(&'static str);

    impl Exporter for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn label(&self) -> &'static str {
            "Fixed"
        }
        fn export(&self, _: &Document, _: &ExportOptions) -> Result<ExportResult, ExportError> {
            Ok(ExportResult {
                target: self.0.into(),
                files: vec![OutputFile {
                    path: "out.txt".into(),
                    contents: String::new(),
                }],
            })
        }
    }

    fn doc() -> Document {
        Document::default()
    }

    #[test]
    fn registry_dispatches_by_id_and_lists_in_order() {
        let mut registry = ExporterRegistry::new();
        registry
            .register(Fixed("a"))
            .unwrap()
            .register(Fixed("b"))
            .unwrap();
        assert_eq!(
            registry.list(),
            [
                TargetInfo {
                    id: "a",
                    label: "Fixed"
                },
                TargetInfo {
                    id: "b",
                    label: "Fixed"
                }
            ]
        );
        assert_eq!(
            registry
                .export("b", &doc(), &ExportOptions::new())
                .unwrap()
                .target,
            "b"
        );
        let err = registry
            .export("cobol", &doc(), &ExportOptions::new())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"unknown export target "cobol"; available: a, b"#
        );
    }

    #[test]
    fn registry_rejects_duplicate_ids() {
        let mut registry = ExporterRegistry::new();
        registry.register(Fixed("a")).unwrap();
        assert!(matches!(
            registry.register(Fixed("a")),
            Err(ExportError::DuplicateTarget(_))
        ));
    }
}
