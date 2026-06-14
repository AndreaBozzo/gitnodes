// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Reading a local working tree into the projection inputs (`RawFile`s +
//! `BrainConfig`). Shared by the read-only MCP server (`mcp.rs`) and the
//! `gitnodes preview` local mode, so both index a directory with identical
//! inclusion rules, size limits, and fingerprinting.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

use gitnodes_domain::BrainConfig;
use gitnodes_graph::{RawFile, is_included_md};
use sha2::{Digest, Sha256};

pub(crate) const MAX_MARKDOWN_BYTES: u64 = 1024 * 1024;
const CONFIG_PATH: &str = ".gitnodes.yml";
const LEGACY_CONFIG_PATH: &str = ".brain-config.yml";

/// One indexable markdown file located by the working-tree scan, before its
/// contents are read. `size` and `mtime` are the cheap signals the fingerprint
/// compares so an unchanged tree is detected without reading every file.
struct ScanEntry {
    rel: String,
    abs: PathBuf,
    size: u64,
    mtime: Option<SystemTime>,
}

/// Outcome of a refresh scan: either nothing changed since the last rebuild, or
/// the working tree was re-read and is ready to project.
pub(crate) enum RefreshScan {
    Unchanged,
    Changed {
        config: BrainConfig,
        files: Vec<RawFile>,
        fingerprint: u64,
    },
}

/// Walk the working tree, fingerprint it, and only re-read file contents when
/// the fingerprint differs from `last`. The stat-only walk is far cheaper than
/// reading every file plus rebuilding FTS, which is the common no-change path.
pub(crate) fn scan_for_refresh(root: &Path, last: Option<u64>) -> Result<RefreshScan, String> {
    let entries = scan_entries(root)?;
    let fingerprint = working_tree_fingerprint(root, &entries);
    if last == Some(fingerprint) {
        return Ok(RefreshScan::Unchanged);
    }
    let config = read_config(root)?;
    let files = read_entries(&entries)?;
    Ok(RefreshScan::Changed {
        config,
        files,
        fingerprint,
    })
}

/// Read the working tree unconditionally into projection inputs. Used on the
/// paths that always want a fresh read (boot seeding and the manual refresh
/// button), where the fingerprint short-circuit of [`scan_for_refresh`] is not
/// needed.
pub(crate) fn read_working_tree(root: &Path) -> Result<(BrainConfig, Vec<RawFile>), String> {
    let entries = scan_entries(root)?;
    let config = read_config(root)?;
    let files = read_entries(&entries)?;
    Ok((config, files))
}

/// Cheap content-independent signature of the working tree: config metadata
/// plus every indexable file's path, size, and mtime. A content edit changes
/// size or mtime; config edits are folded in because configuration shapes the
/// projection even though it is not itself an indexed node.
fn working_tree_fingerprint(root: &Path, entries: &[ScanEntry]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for config_path in [CONFIG_PATH, LEGACY_CONFIG_PATH] {
        config_path.hash(&mut hasher);
        match std::fs::metadata(root.join(config_path)) {
            Ok(meta) => {
                meta.len().hash(&mut hasher);
                hash_mtime(meta.modified().ok(), &mut hasher);
            }
            // Distinguish "no config" from a present-but-empty config.
            Err(_) => u64::MAX.hash(&mut hasher),
        }
    }
    for entry in entries {
        entry.rel.hash(&mut hasher);
        entry.size.hash(&mut hasher);
        hash_mtime(entry.mtime, &mut hasher);
    }
    hasher.finish()
}

fn hash_mtime(mtime: Option<SystemTime>, hasher: &mut DefaultHasher) {
    mtime
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos())
        .hash(hasher);
}

pub(crate) fn read_config(root: &Path) -> Result<BrainConfig, String> {
    for name in [CONFIG_PATH, LEGACY_CONFIG_PATH] {
        let config_path = root.join(name);
        if config_path.is_file() {
            let source = std::fs::read_to_string(&config_path)
                .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
            return BrainConfig::parse(&source)
                .map_err(|error| format!("{} is invalid: {error}", config_path.display()));
        }
    }
    Ok(BrainConfig::default())
}

fn read_entries(entries: &[ScanEntry]) -> Result<Vec<RawFile>, String> {
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = std::fs::read_to_string(&entry.abs)
            .map_err(|error| format!("failed to read {} as UTF-8: {error}", entry.abs.display()))?;
        let sha = format!("{:x}", Sha256::digest(content.as_bytes()));
        files.push(RawFile {
            path: entry.rel.clone(),
            sha,
            content,
        });
    }
    Ok(files)
}

fn scan_entries(root: &Path) -> Result<Vec<ScanEntry>, String> {
    let mut entries = Vec::new();
    collect_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.rel.cmp(&right.rel));
    Ok(entries)
}

fn collect_entries(root: &Path, current: &Path, out: &mut Vec<ScanEntry>) -> Result<(), String> {
    let dir = std::fs::read_dir(current)
        .map_err(|error| format!("failed to scan {}: {error}", current.display()))?;
    for entry in dir {
        let entry =
            entry.map_err(|error| format!("failed to read {}: {error}", current.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || matches!(name.as_ref(), "data" | "node_modules" | "target")
            {
                continue;
            }
            collect_entries(root, &path, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("failed to relativize {}: {error}", path.display()))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !is_included_md(&relative) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        let size = metadata.len();
        if size > MAX_MARKDOWN_BYTES {
            return Err(format!(
                "{} is larger than the 1 MiB local indexing limit",
                path.display()
            ));
        }
        out.push(ScanEntry {
            rel: relative,
            abs: path,
            size,
            mtime: metadata.modified().ok(),
        });
    }
    Ok(())
}

pub(crate) fn read_confined_markdown(root: &Path, relative: &str) -> Result<String, String> {
    if !is_included_md(relative) {
        return Err(format!("not an indexable markdown path: {relative}"));
    }
    let candidate = std::fs::canonicalize(root.join(relative))
        .map_err(|error| format!("failed to open {relative}: {error}"))?;
    if !candidate.starts_with(root) {
        return Err("path escapes the knowledge directory".to_string());
    }
    std::fs::read_to_string(&candidate)
        .map_err(|error| format!("failed to read {}: {error}", candidate.display()))
}

#[cfg(test)]
mod tests {
    use super::{RefreshScan, read_confined_markdown, read_working_tree, scan_for_refresh};
    use std::path::{Path, PathBuf};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "gitnodes-working-tree-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock should be after the Unix epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn working_tree_scan_uses_graph_inclusion_rules() {
        let dir = TestDir::new();
        std::fs::create_dir_all(dir.path().join("concepts")).expect("create concepts");
        std::fs::create_dir_all(dir.path().join(".private")).expect("create hidden directory");
        std::fs::write(
            dir.path().join("concepts/search.md"),
            "---\ntype: concept\ntopic: search\n---\nUseful body.\n",
        )
        .expect("write node");
        std::fs::write(dir.path().join("README.md"), "# ignored").expect("write readme");
        std::fs::write(dir.path().join(".private/secret.md"), "# ignored")
            .expect("write hidden node");

        let (_config, files) = read_working_tree(dir.path()).expect("read working tree");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "concepts/search.md");
    }

    #[test]
    fn direct_reads_reject_paths_outside_the_brain() {
        let dir = TestDir::new();
        let error = read_confined_markdown(dir.path(), "../outside.md")
            .expect_err("traversal should be rejected");
        assert!(error.contains("not an indexable markdown path"));
    }

    #[test]
    fn refresh_scan_detects_changes_and_skips_unchanged_trees() {
        let dir = TestDir::new();
        std::fs::create_dir_all(dir.path().join("concepts")).expect("create concepts");
        let note = dir.path().join("concepts/a.md");
        std::fs::write(&note, "---\ntype: concept\ntopic: a\n---\nbody\n").expect("write note");

        let fingerprint = match scan_for_refresh(dir.path(), None).expect("first scan") {
            RefreshScan::Changed { fingerprint, .. } => fingerprint,
            RefreshScan::Unchanged => panic!("first scan must rebuild"),
        };

        // Nothing changed on disk: the rescan must short-circuit.
        assert!(matches!(
            scan_for_refresh(dir.path(), Some(fingerprint)).expect("unchanged scan"),
            RefreshScan::Unchanged
        ));

        // A content edit changes the file size, so the fingerprint must differ
        // regardless of mtime resolution.
        std::fs::write(
            &note,
            "---\ntype: concept\ntopic: a\n---\nbody, now longer\n",
        )
        .expect("edit note");
        assert!(matches!(
            scan_for_refresh(dir.path(), Some(fingerprint)).expect("changed scan"),
            RefreshScan::Changed { .. }
        ));
    }

    #[test]
    fn legacy_config_is_loaded_and_fingerprinted() {
        let dir = TestDir::new();
        let legacy = dir.path().join(".brain-config.yml");
        std::fs::write(
            &legacy,
            "default_type: note\nnode_types:\n  - name: note\n    label: Note\n    directory: notes\n    accent: \"#112233\"\n",
        )
        .expect("write legacy config");

        let first = match scan_for_refresh(dir.path(), None).expect("first scan") {
            RefreshScan::Changed {
                config,
                fingerprint,
                ..
            } => {
                assert!(config.lookup("note").is_some());
                fingerprint
            }
            RefreshScan::Unchanged => panic!("first scan must rebuild"),
        };

        std::fs::write(
            &legacy,
            "default_type: long-memo\nnode_types:\n  - name: long-memo\n    label: Long Memo\n    directory: long-memos\n    accent: \"#445566\"\n",
        )
        .expect("edit legacy config");
        assert!(matches!(
            scan_for_refresh(dir.path(), Some(first)).expect("legacy config changed"),
            RefreshScan::Changed { .. }
        ));
    }
}
