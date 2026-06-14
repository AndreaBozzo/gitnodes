// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Read-only validation for local GitNodes working trees.

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

use gitnodes_domain::{BrainConfig, EdgeKind, split_frontmatter};
use gitnodes_graph::{RawFile, parse_file};
use serde::Serialize;
use serde_yaml::Value;

use crate::server::working_tree::{read_config, read_markdown_files};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidationDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ValidationReport {
    pub files_scanned: usize,
    pub nodes_valid: usize,
    pub errors: usize,
    pub warnings: usize,
    pub diagnostics: Vec<ValidationDiagnostic>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors == 0
    }

    fn push(
        &mut self,
        severity: DiagnosticSeverity,
        code: &'static str,
        path: Option<&str>,
        message: impl Into<String>,
    ) {
        match severity {
            DiagnosticSeverity::Error => self.errors += 1,
            DiagnosticSeverity::Warning => self.warnings += 1,
        }
        self.diagnostics.push(ValidationDiagnostic {
            severity,
            code,
            path: path.map(ToOwned::to_owned),
            message: message.into(),
        });
    }
}

/// Validate a local working tree without mutating it or opening the projection.
pub fn validate_working_tree(root: &Path) -> Result<ValidationReport, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }

    let files = read_markdown_files(root)?;
    match read_config(root) {
        Ok(config) => Ok(validate_raw_files(&config, &files)),
        Err(error) => {
            let mut report = validate_raw_files(&BrainConfig::default(), &files);
            report.push(DiagnosticSeverity::Error, "config_invalid", None, error);
            Ok(report)
        }
    }
}

pub fn validate_raw_files(config: &BrainConfig, files: &[RawFile]) -> ValidationReport {
    let mut report = ValidationReport {
        files_scanned: files.len(),
        ..ValidationReport::default()
    };
    let paths: HashSet<&str> = files.iter().map(|file| file.path.as_str()).collect();

    for file in files {
        validate_file(config, file, &paths, &mut report);
    }

    if files.is_empty() {
        report.push(
            DiagnosticSeverity::Warning,
            "brain_empty",
            None,
            "no indexable markdown files were found",
        );
    } else if report.nodes_valid == 0 {
        report.push(
            DiagnosticSeverity::Warning,
            "no_valid_nodes",
            None,
            "no markdown files contain valid GitNodes frontmatter",
        );
    }

    report
}

fn validate_file(
    config: &BrainConfig,
    file: &RawFile,
    paths: &HashSet<&str>,
    report: &mut ValidationReport,
) {
    let starts_frontmatter =
        file.content.starts_with("---\n") || file.content.starts_with("---\r\n");
    let (front, _) = split_frontmatter(&file.content);
    if front.is_empty() {
        let (code, message) = if starts_frontmatter {
            (
                "frontmatter_unterminated",
                "frontmatter starts with `---` but has no closing fence",
            )
        } else {
            (
                "frontmatter_missing",
                "file has no YAML frontmatter and will not appear as a graph node",
            )
        };
        report.push(
            if starts_frontmatter {
                DiagnosticSeverity::Error
            } else {
                DiagnosticSeverity::Warning
            },
            code,
            Some(&file.path),
            message,
        );
        return;
    }

    let frontmatter = match serde_yaml::from_str::<Value>(front) {
        Ok(Value::Mapping(map)) => map,
        Ok(_) => {
            report.push(
                DiagnosticSeverity::Error,
                "frontmatter_not_mapping",
                Some(&file.path),
                "frontmatter must be a YAML mapping",
            );
            return;
        }
        Err(error) => {
            report.push(
                DiagnosticSeverity::Error,
                "frontmatter_invalid",
                Some(&file.path),
                format!("invalid YAML frontmatter: {error}"),
            );
            return;
        }
    };

    let type_value = frontmatter.get(Value::String("type".to_string()));
    let Some(node_type) = type_value.and_then(Value::as_str).map(str::trim) else {
        report.push(
            DiagnosticSeverity::Error,
            "type_missing",
            Some(&file.path),
            "`type` must be a non-empty string",
        );
        return;
    };
    if node_type.is_empty() {
        report.push(
            DiagnosticSeverity::Error,
            "type_missing",
            Some(&file.path),
            "`type` must be a non-empty string",
        );
        return;
    }

    if let Some(spec) = config.lookup(node_type) {
        if !spec.directory.is_empty()
            && !path_is_within_directory(&file.path, spec.directory.as_str())
        {
            report.push(
                DiagnosticSeverity::Warning,
                "type_directory_mismatch",
                Some(&file.path),
                format!("type `{node_type}` is configured for `{}/`", spec.directory),
            );
        }
    } else {
        report.push(
            DiagnosticSeverity::Warning,
            "type_unknown",
            Some(&file.path),
            format!(
                "unknown node type `{node_type}`; the UI will style it as `{}`",
                config.default_type
            ),
        );
    }

    if let Some(tags) = frontmatter.get(Value::String("tags".to_string()))
        && !valid_tags(tags)
    {
        report.push(
            DiagnosticSeverity::Warning,
            "tags_invalid",
            Some(&file.path),
            "`tags` should be a sequence of non-empty strings",
        );
    }

    let Some(parsed) = parse_file(&file.content, &file.path, &file.sha, config) else {
        report.push(
            DiagnosticSeverity::Error,
            "node_unparseable",
            Some(&file.path),
            "file could not be parsed as a GitNodes node",
        );
        return;
    };
    report.nodes_valid += 1;

    let from_dir = Path::new(&file.path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    for link in parsed.links {
        let target = match &link.kind {
            EdgeKind::Body => normalize_link(from_dir, &link.target),
            EdgeKind::Frontmatter(_) | EdgeKind::Tag => Some(link.target.clone()),
        };
        let Some(target) = target else {
            report.push(
                DiagnosticSeverity::Warning,
                "link_invalid",
                Some(&file.path),
                format!("link target `{}` cannot be normalized", link.target),
            );
            continue;
        };
        if !paths.contains(target.as_str()) {
            let kind = match link.kind {
                EdgeKind::Body => "body",
                EdgeKind::Frontmatter(_) => "frontmatter",
                EdgeKind::Tag => continue,
            };
            report.push(
                DiagnosticSeverity::Warning,
                "link_unresolved",
                Some(&file.path),
                format!("{kind} link target `{target}` does not exist"),
            );
        }
    }
}

fn path_is_within_directory(path: &str, directory: &str) -> bool {
    path == directory || path.starts_with(&format!("{directory}/"))
}

fn valid_tags(tags: &Value) -> bool {
    tags.as_sequence().is_some_and(|values| {
        values
            .iter()
            .all(|value| value.as_str().is_some_and(|tag| !tag.trim().is_empty()))
    })
}

fn normalize_link(from_dir: &Path, link: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in from_dir.join(link).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(
        PathBuf::from_iter(parts)
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: &str) -> RawFile {
        RawFile {
            path: path.to_string(),
            sha: "sha".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn valid_brain_reports_nodes_and_unresolved_links() {
        let files = vec![
            file(
                "concepts/one.md",
                "---\ntype: concept\ntopic: one\ntags: [test]\n---\n[Two](two.md)\n[Gone](gone.md)",
            ),
            file("concepts/two.md", "---\ntype: concept\ntopic: two\n---\n"),
        ];

        let report = validate_raw_files(&BrainConfig::default(), &files);
        assert_eq!(report.nodes_valid, 2);
        assert_eq!(report.errors, 0);
        assert_eq!(report.warnings, 1);
        assert_eq!(report.diagnostics[0].code, "link_unresolved");
    }

    #[test]
    fn malformed_nodes_are_errors_but_plain_markdown_is_a_warning() {
        let files = vec![
            file("concepts/plain.md", "# Plain"),
            file("concepts/broken.md", "---\ntype: concept\n"),
            file("concepts/no-type.md", "---\ntopic: Missing\n---\n"),
        ];

        let report = validate_raw_files(&BrainConfig::default(), &files);
        assert_eq!(report.nodes_valid, 0);
        assert_eq!(report.errors, 2);
        assert_eq!(report.warnings, 2);
    }

    #[test]
    fn warns_on_unknown_type_directory_and_tags_shape() {
        let files = vec![
            file("projects/wrong.md", "---\ntype: concept\n---\n"),
            file("concepts/custom.md", "---\ntype: custom\ntags: nope\n---\n"),
        ];

        let report = validate_raw_files(&BrainConfig::default(), &files);
        let codes = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"type_directory_mismatch"));
        assert!(codes.contains(&"type_unknown"));
        assert!(codes.contains(&"tags_invalid"));
    }
}
