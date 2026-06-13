//! Read-only MCP access to a local GitNodes working tree.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::SystemTime,
};

use gitnodes_domain::{BrainConfig, TargetConfig};
use gitnodes_graph::{RawFile, is_included_md};
use rmcp::{
    ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tokio::sync::Mutex;

use crate::server::projection;

const MAX_MARKDOWN_BYTES: u64 = 1024 * 1024;
const DEFAULT_LIST_LIMIT: usize = 100;
const HARD_LIST_LIMIT: usize = 500;

#[derive(Clone, Debug)]
struct GitNodesMcp {
    root: Arc<PathBuf>,
    target: TargetConfig,
    refresh_lock: Arc<Mutex<()>>,
    /// Fingerprint of the working tree behind the last rebuild. Lets `refresh`
    /// skip the rescan-and-rebuild when nothing on disk has changed, so an agent
    /// doing many tool calls in a row doesn't rebuild the projection each time.
    last_fingerprint: Arc<std::sync::Mutex<Option<u64>>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchBrainParams {
    /// Full-text query. Plain words and quoted phrases are supported.
    query: String,
    /// Restrict results to these configured node type names.
    #[serde(default)]
    node_types: Vec<String>,
    /// Keep nodes containing at least one of these tags.
    #[serde(default)]
    tags: Vec<String>,
    /// Restrict results to a repository-relative directory.
    path_prefix: Option<String>,
    /// Maximum number of results (1-100).
    limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchBrainResult {
    path: String,
    title: String,
    snippet: String,
    score: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchBrainResponse {
    results: Vec<SearchBrainResult>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListNodesParams {
    /// Restrict results to these configured node type names.
    #[serde(default)]
    node_types: Vec<String>,
    /// Keep nodes containing at least one of these tags.
    #[serde(default)]
    tags: Vec<String>,
    /// Restrict results to a repository-relative directory.
    path_prefix: Option<String>,
    /// Include generated tag nodes as well as markdown files.
    #[serde(default)]
    include_virtual: bool,
    /// Maximum number of results (1-500).
    limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct NodeSummary {
    path: String,
    title: String,
    summary: String,
    node_type: String,
    tags: Vec<String>,
    is_virtual: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListNodesResponse {
    nodes: Vec<NodeSummary>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadNodeParams {
    /// Repository-relative markdown path returned by search_brain or list_nodes.
    path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct NodeDocument {
    path: String,
    title: String,
    summary: String,
    node_type: String,
    tags: Vec<String>,
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NodeLinksParams {
    /// Repository-relative markdown path returned by search_brain or list_nodes.
    path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct LinkedNode {
    path: String,
    title: String,
    node_type: String,
    /// "outgoing" (this node links out) or "incoming" (the other node links here).
    direction: String,
    /// Edge kind: "body" link, "frontmatter" link, or shared "tag".
    kind: String,
    /// True for generated tag nodes, which have no markdown file of their own.
    is_virtual: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
struct NodeLinksResponse {
    path: String,
    links: Vec<LinkedNode>,
}

#[tool_router(server_handler)]
impl GitNodesMcp {
    #[tool(
        name = "search_brain",
        description = "Search the current GitNodes working tree using the same FTS5 projection and ranking as the web UI."
    )]
    async fn search_brain(
        &self,
        Parameters(params): Parameters<SearchBrainParams>,
    ) -> Result<Json<SearchBrainResponse>, String> {
        self.refresh().await?;
        let hits = projection::search_nodes(
            &self.target,
            &projection::SearchFilters {
                q: params.query,
                node_types: params.node_types,
                tags: params.tags,
                path_prefix: params.path_prefix,
                limit: params.limit,
            },
        )
        .await
        .map_err(|error| error.to_string())?;

        Ok(Json(SearchBrainResponse {
            results: hits
                .into_iter()
                .map(|hit| SearchBrainResult {
                    path: hit.path,
                    title: hit.title,
                    snippet: hit.snippet,
                    score: hit.score,
                })
                .collect(),
        }))
    }

    #[tool(
        name = "list_nodes",
        description = "List structured nodes from the current GitNodes working tree, optionally filtered by type, tag, or directory."
    )]
    async fn list_nodes(
        &self,
        Parameters(params): Parameters<ListNodesParams>,
    ) -> Result<Json<ListNodesResponse>, String> {
        self.refresh().await?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, HARD_LIST_LIMIT);
        let nodes = projection::list_nodes(
            &self.target,
            &projection::NodeFilters {
                node_types: params.node_types,
                tags: params.tags,
                paths: Vec::new(),
                path_prefix: params.path_prefix,
                include_virtual: params.include_virtual,
                limit: Some(limit),
            },
        )
        .await
        .map_err(|error| error.to_string())?;

        Ok(Json(ListNodesResponse {
            nodes: nodes
                .into_iter()
                .take(limit)
                .map(|node| NodeSummary {
                    is_virtual: node.path.is_empty(),
                    path: node.path,
                    title: node.title,
                    summary: node.summary,
                    node_type: node.node_type,
                    tags: node.tags,
                })
                .collect(),
        }))
    }

    #[tool(
        name = "read_node",
        description = "Read one markdown node and its projected metadata from the current GitNodes working tree."
    )]
    async fn read_node(
        &self,
        Parameters(params): Parameters<ReadNodeParams>,
    ) -> Result<Json<NodeDocument>, String> {
        self.refresh().await?;
        let node = projection::read_node(&self.target, &params.path)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("node not found: {}", params.path))?;
        let content = read_confined_markdown(&self.root, &params.path)?;

        Ok(Json(NodeDocument {
            path: node.path,
            title: node.title,
            summary: node.summary,
            node_type: node.node_type,
            tags: node.tags,
            content,
        }))
    }

    #[tool(
        name = "node_links",
        description = "List the typed graph edges touching one node — incoming and outgoing body links, frontmatter links, and shared-tag connections — so an agent can traverse the GitNodes knowledge graph instead of grepping."
    )]
    async fn node_links(
        &self,
        Parameters(params): Parameters<NodeLinksParams>,
    ) -> Result<Json<NodeLinksResponse>, String> {
        self.refresh().await?;
        let neighbors = projection::node_neighbors(&self.target, &params.path)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("node not found: {}", params.path))?;

        Ok(Json(NodeLinksResponse {
            links: neighbors
                .into_iter()
                .map(|neighbor| LinkedNode {
                    path: neighbor.path,
                    title: neighbor.title,
                    node_type: neighbor.node_type,
                    direction: neighbor.direction.to_string(),
                    kind: neighbor.kind,
                    is_virtual: neighbor.is_virtual,
                })
                .collect(),
            path: params.path,
        }))
    }
}

impl GitNodesMcp {
    async fn refresh(&self) -> Result<(), String> {
        let _guard = self.refresh_lock.lock().await;
        let root = Arc::clone(&self.root);
        let last = *self
            .last_fingerprint
            .lock()
            .expect("fingerprint mutex poisoned");
        let scan = tokio::task::spawn_blocking(move || scan_for_refresh(&root, last))
            .await
            .map_err(|error| format!("local index task failed: {error}"))??;
        match scan {
            RefreshScan::Unchanged => Ok(()),
            RefreshScan::Changed {
                config,
                files,
                fingerprint,
            } => {
                projection::rebuild_from_raw_files(
                    &self.target,
                    &files,
                    &config,
                    "mcp-working-tree",
                )
                .await
                .map_err(|error| error.to_string())?;
                *self
                    .last_fingerprint
                    .lock()
                    .expect("fingerprint mutex poisoned") = Some(fingerprint);
                Ok(())
            }
        }
    }
}

/// Start a stdio MCP server for `dir`, or the current directory when omitted.
pub async fn run(dir: Option<&str>) -> Result<(), String> {
    let root = std::fs::canonicalize(dir.unwrap_or("."))
        .map_err(|error| format!("failed to open local knowledge directory: {error}"))?;
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }

    let repo = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("working-tree")
        .to_string();
    let target = TargetConfig {
        org: "_local".to_string(),
        repo,
        branch: "working-tree".to_string(),
    };

    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|error| format!("failed to configure local projection: {error}"))?
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| format!("failed to open local projection: {error}"))?;
    projection::migrate(&pool)
        .await
        .map_err(|error| format!("failed to migrate local projection: {error}"))?;
    projection::init(pool);

    let server = GitNodesMcp {
        root: Arc::new(root),
        target,
        refresh_lock: Arc::new(Mutex::new(())),
        last_fingerprint: Arc::new(std::sync::Mutex::new(None)),
    };
    server.refresh().await?;
    server
        .serve(stdio())
        .await
        .map_err(|error| format!("failed to initialize MCP transport: {error}"))?
        .waiting()
        .await
        .map_err(|error| format!("MCP transport task failed: {error}"))?;
    Ok(())
}

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
enum RefreshScan {
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
fn scan_for_refresh(root: &Path, last: Option<u64>) -> Result<RefreshScan, String> {
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

/// Cheap content-independent signature of the working tree: the config file's
/// size and mtime plus every indexable file's path, size, and mtime. A content
/// edit changes size or mtime; a config edit is folded in because `.gitnodes.yml`
/// shapes the projection even though it is not itself an indexed node.
fn working_tree_fingerprint(root: &Path, entries: &[ScanEntry]) -> u64 {
    let mut hasher = DefaultHasher::new();
    match std::fs::metadata(root.join(".gitnodes.yml")) {
        Ok(meta) => {
            meta.len().hash(&mut hasher);
            hash_mtime(meta.modified().ok(), &mut hasher);
        }
        // Distinguish "no config" from a present-but-empty config.
        Err(_) => u64::MAX.hash(&mut hasher),
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

fn read_config(root: &Path) -> Result<BrainConfig, String> {
    let config_path = root.join(".gitnodes.yml");
    if config_path.is_file() {
        let source = std::fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
        BrainConfig::parse(&source)
            .map_err(|error| format!("{} is invalid: {error}", config_path.display()))
    } else {
        Ok(BrainConfig::default())
    }
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

fn read_confined_markdown(root: &Path, relative: &str) -> Result<String, String> {
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
    use super::{
        GitNodesMcp, RefreshScan, read_confined_markdown, read_entries, scan_entries,
        scan_for_refresh,
    };
    use gitnodes_domain::TargetConfig;
    use rmcp::ServiceExt;
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        sync::Mutex,
    };

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "gitnodes-mcp-{}-{}",
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

        let files = read_entries(&scan_entries(dir.path()).expect("scan working tree"))
            .expect("read entries");

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

    #[tokio::test]
    async fn protocol_lists_all_read_only_tools() {
        let dir = TestDir::new();
        let server = GitNodesMcp {
            root: Arc::new(dir.path().to_path_buf()),
            target: TargetConfig {
                org: "_local".to_string(),
                repo: "test".to_string(),
                branch: "working-tree".to_string(),
            },
            refresh_lock: Arc::new(Mutex::new(())),
            last_fingerprint: Arc::new(std::sync::Mutex::new(None)),
        };
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            server
                .serve(server_transport)
                .await
                .expect("start MCP server")
                .waiting()
                .await
                .expect("wait for MCP server");
        });
        let (reader, mut writer) = tokio::io::split(client_transport);
        let mut reader = BufReader::new(reader);

        writer
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n",
            )
            .await
            .expect("send initialize");
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("read initialize response");
        let initialized: serde_json::Value =
            serde_json::from_str(&line).expect("parse initialize response");
        assert_eq!(initialized["id"], 1);

        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .expect("send initialized notification");
        tokio::task::yield_now().await;
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n")
            .await
            .expect("send tools/list");
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .expect("read tools/list response");
        let listed: serde_json::Value =
            serde_json::from_str(&line).expect("parse tools/list response");
        let mut names = listed["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(
            names,
            ["list_nodes", "node_links", "read_node", "search_brain"]
        );

        drop(writer);
        drop(reader);
        server_task.await.expect("join MCP server task");
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
}
