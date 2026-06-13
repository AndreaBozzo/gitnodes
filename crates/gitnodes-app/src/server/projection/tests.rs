use std::str::FromStr;

use gitnodes_domain::{BrainConfig, TargetConfig, WorkItemState, WorkItemSystemOfRecord};
use gitnodes_graph::RawFile;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use super::{
    FileFilters, NodeFilters, WorkItemFilters,
    files::list_files_from_pool,
    migrations::migrate,
    nodes::{list_nodes_from_pool, load_cached_graph, node_neighbors_from_pool},
    pending_sync,
    rebuild::{ProjectionSnapshot, persist_snapshot},
    search::{SearchFilters, search_nodes_from_pool},
    sync_state::load_sync_state,
    target::ensure_target_id,
    work_items::{list_work_items_from_pool, load_work_item_by_path_from_pool},
};

fn target(org: &str, repo: &str, branch: &str) -> TargetConfig {
    TargetConfig {
        org: org.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
    }
}

fn raw(path: &str, sha: &str, content: &str) -> RawFile {
    RawFile {
        path: path.to_string(),
        sha: sha.to_string(),
        content: content.to_string(),
    }
}

async fn test_pool() -> sqlx::SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn snapshot_persists_per_target_without_cross_talk() {
    let pool = test_pool().await;
    let config = BrainConfig::default();

    let snapshot_a = ProjectionSnapshot::from_raw_files(
        &[raw(
            "concepts/A.md",
            "sha-a",
            "---\ntype: concept\ntopic: A\n---\nsee [B](./B.md)\n",
        )],
        &config,
    );
    let snapshot_b = ProjectionSnapshot::from_raw_files(
        &[raw(
            "concepts/B.md",
            "sha-b",
            "---\ntype: concept\ntopic: B\n---\nbody\n",
        )],
        &config,
    );

    let target_a = target("org", "repo-a", "main");
    let target_b = target("org", "repo-b", "main");
    let target_a_id = ensure_target_id(&pool, &target_a).await.unwrap();
    let target_b_id = ensure_target_id(&pool, &target_b).await.unwrap();

    persist_snapshot(&pool, target_a_id, &snapshot_a, "test-a")
        .await
        .unwrap();
    persist_snapshot(&pool, target_b_id, &snapshot_b, "test-b")
        .await
        .unwrap();

    let (nodes_a, _) = load_cached_graph(&pool, target_a_id).await.unwrap();
    let (nodes_b, _) = load_cached_graph(&pool, target_b_id).await.unwrap();

    assert_eq!(nodes_a.len(), 1);
    assert_eq!(nodes_b.len(), 1);
    assert_eq!(nodes_a[0].title, "A");
    assert_eq!(nodes_b[0].title, "B");
}

#[tokio::test]
async fn snapshot_materializes_work_items_and_bindings() {
    let pool = test_pool().await;
    let config = BrainConfig::parse(
        r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#fb7185"
    title_key: topic
    work_item_kind: task
"##,
    )
    .unwrap();

    let snapshot = ProjectionSnapshot::from_raw_files(
        &[raw(
            "tasks/stabilize-sync.md",
            "sha-task",
            "---\ntype: task\ntopic: Stabilize sync\nbrain_id: task-sync-1\nstate: in-progress\nassignees: [alice, bob]\nexternal_binding:\n  system: github\n  project: AndreaBozzo/Brain_UI\n  item_key: \"42\"\n  url: https://github.com/AndreaBozzo/Brain_UI/issues/42\n---\n# Task: Stabilize sync\n\n## Description\nBody\n",
        )],
        &config,
    );

    let target_id = ensure_target_id(&pool, &target("org", "repo-workitems", "main"))
        .await
        .unwrap();
    persist_snapshot(&pool, target_id, &snapshot, "test-work-items")
        .await
        .unwrap();

    let item = sqlx::query(
        "SELECT brain_id, kind, title, state, labels_json, assignees_json, content_path, system_of_record FROM work_items WHERE target_id = ?",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(item.get::<String, _>("brain_id"), "task-sync-1");
    assert_eq!(item.get::<String, _>("kind"), "task");
    assert_eq!(item.get::<String, _>("title"), "Stabilize Sync");
    assert_eq!(item.get::<String, _>("state"), "in-progress");
    assert_eq!(
        item.get::<String, _>("content_path"),
        "tasks/stabilize-sync.md"
    );
    assert_eq!(item.get::<String, _>("system_of_record"), "split");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&item.get::<String, _>("labels_json")).unwrap(),
        vec!["brain:task".to_string(), "brain:in-progress".to_string()]
    );
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&item.get::<String, _>("assignees_json")).unwrap(),
        vec!["alice".to_string(), "bob".to_string()]
    );

    let binding = sqlx::query(
        "SELECT system, project, item_key, url FROM work_item_bindings WHERE target_id = ? AND brain_id = ?",
    )
    .bind(target_id)
    .bind("task-sync-1")
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(binding.get::<String, _>("system"), "github");
    assert_eq!(binding.get::<String, _>("project"), "AndreaBozzo/Brain_UI");
    assert_eq!(binding.get::<String, _>("item_key"), "42");
    assert_eq!(
        binding.get::<String, _>("url"),
        "https://github.com/AndreaBozzo/Brain_UI/issues/42"
    );
}

#[tokio::test]
async fn load_work_item_by_path_reads_projected_binding() {
    let pool = test_pool().await;
    let config = BrainConfig::parse(
        r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#fb7185"
    title_key: topic
    work_item_kind: task
"##,
    )
    .unwrap();

    let snapshot = ProjectionSnapshot::from_raw_files(
        &[raw(
            "tasks/api-read.md",
            "sha-task",
            "---\ntype: task\ntopic: API read\nbrain_id: task-api-1\nstatus: done\nassignees: [andrea]\nexternal_binding:\n  system: github\n  project: AndreaBozzo/Brain_UI\n  item_key: \"77\"\n  provider_id: I_kwDO123\n  url: https://github.com/AndreaBozzo/Brain_UI/issues/77\nsystem_of_record: split\n---\n# Task: API read\n",
        )],
        &config,
    );
    let target = target("org", "repo-workitems-read", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    persist_snapshot(&pool, target_id, &snapshot, "test-work-item-read")
        .await
        .unwrap();

    let item = load_work_item_by_path_from_pool(&pool, &target, "tasks/api-read.md")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(item.brain_id, "task-api-1");
    assert_eq!(item.state, WorkItemState::Done);
    assert_eq!(item.assignees, vec!["andrea".to_string()]);
    assert_eq!(item.system_of_record, WorkItemSystemOfRecord::Split);
    assert!(item.labels.contains(&"brain:task".to_string()));
    let binding = item.external_binding.expect("binding must exist");
    assert_eq!(binding.item_key, "77");
    assert_eq!(binding.project, "AndreaBozzo/Brain_UI");
}

#[tokio::test]
async fn snapshot_materializes_backlinks_and_sync_state() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: A\n---\nsee [B](./B.md)\n",
            ),
            raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: B\n---\nbody\n",
            ),
        ],
        &config,
    );

    persist_snapshot(&pool, target_id, &snapshot, "bootstrap")
        .await
        .unwrap();

    let sync = load_sync_state(&pool, target_id).await.unwrap().unwrap();
    let backlink_count = sqlx::query("SELECT COUNT(*) AS count FROM backlinks WHERE target_id = ?")
        .bind(target_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get::<i64, _>("count");

    assert_eq!(sync.status, "ready");
    assert!(sync.last_success_at.is_some());
    assert_eq!(backlink_count, 1);
}

#[tokio::test]
async fn node_neighbors_resolves_edges_in_both_directions() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-neighbors", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: Alpha\n---\nsee [B](./B.md)\n",
            ),
            raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: Beta\n---\nbody\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-neighbors")
        .await
        .unwrap();

    // A links out to B.
    let from_a = node_neighbors_from_pool(&pool, target_id, "concepts/A.md")
        .await
        .unwrap()
        .expect("A is a node");
    assert!(
        from_a
            .iter()
            .any(|n| n.path == "concepts/B.md" && n.direction == "outgoing"),
        "A should have an outgoing edge to B: {from_a:?}"
    );

    // B sees the same edge as incoming.
    let into_b = node_neighbors_from_pool(&pool, target_id, "concepts/B.md")
        .await
        .unwrap()
        .expect("B is a node");
    assert!(
        into_b
            .iter()
            .any(|n| n.path == "concepts/A.md" && n.direction == "incoming"),
        "B should have an incoming edge from A: {into_b:?}"
    );

    // An unknown path is None, not an empty edge list.
    assert!(
        node_neighbors_from_pool(&pool, target_id, "concepts/missing.md")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn list_nodes_limit_caps_results_without_a_tag_filter() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-limit", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: Alpha\ntags: [k]\n---\nbody\n",
            ),
            raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: Beta\ntags: [k]\n---\nbody\n",
            ),
            raw(
                "concepts/C.md",
                "sha-c",
                "---\ntype: concept\ntopic: Gamma\ntags: [k]\n---\nbody\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-limit")
        .await
        .unwrap();

    // No tag filter: the limit is pushed into SQL and caps the rows returned.
    let capped = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            include_virtual: false,
            limit: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(capped.len(), 2);

    // With a tag filter the SQL limit is suppressed (tags are matched in Rust),
    // so all three tagged nodes come back for the caller to bound itself.
    let tagged = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            tags: vec!["k".to_string()],
            include_virtual: false,
            limit: Some(2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(tagged.len(), 3);
}

#[tokio::test]
async fn list_nodes_filters_projection_without_rebuild() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-node-query", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: Alpha\ntags: [sync]\n---\nsee [B](./B.md)\n",
            ),
            raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: Beta\ntags: [sync]\n---\nbody\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-node-query")
        .await
        .unwrap();

    let non_virtual = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            include_virtual: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let tagged = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            tags: vec!["SYNC".to_string()],
            include_virtual: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let beta = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            paths: vec!["concepts/B.md".to_string()],
            include_virtual: false,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .next()
    .unwrap();
    let prefixed = list_nodes_from_pool(
        &pool,
        target_id,
        &NodeFilters {
            path_prefix: Some("concepts".to_string()),
            include_virtual: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(non_virtual.len(), 2);
    assert_eq!(tagged.len(), 2);
    assert_eq!(beta.title, "Beta");
    assert_eq!(prefixed.len(), 2);
}

#[tokio::test]
async fn list_files_reports_structure_metadata() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-file-query", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: Alpha\n---\nsee [B](./B.md)\n",
            ),
            raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: Beta\n---\nbody\n",
            ),
            raw(
                "runbooks/solo.md",
                "sha-c",
                "---\ntype: runbook\ntopic: Solo\n---\nbody\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-file-query")
        .await
        .unwrap();

    let concepts = list_files_from_pool(
        &pool,
        target_id,
        &FileFilters {
            path_prefix: Some("concepts".to_string()),
            orphan_only: false,
        },
    )
    .await
    .unwrap();
    let orphan = list_files_from_pool(
        &pool,
        target_id,
        &FileFilters {
            path_prefix: None,
            orphan_only: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(concepts.len(), 2);
    assert_eq!(concepts[0].title.as_deref(), Some("Alpha"));
    assert!(!concepts[0].is_orphan_in_graph);
    assert_eq!(orphan.len(), 1);
    assert_eq!(orphan[0].path, "runbooks/solo.md");
    assert!(orphan[0].is_orphan_in_graph);
}

#[tokio::test]
async fn migration_creates_usable_fts_table() {
    let pool = test_pool().await;

    sqlx::query("INSERT INTO node_search_fts (target_id, node_id, path, node_type, title, tags, body_text) VALUES (1, 1, 'concepts/a.md', 'concept', 'Alpha', '[]', 'replica drift recovery')")
        .execute(&pool)
        .await
        .unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM node_search_fts WHERE node_search_fts MATCH ?")
            .bind("\"replica\"")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn search_indexes_title_body_and_tags_with_snippets() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-search", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/replica.md",
                "sha-a",
                "---\ntype: concept\ntopic: Replica Drift\ntags: [sync, priority]\n---\nA recovery note about replica drift after webhook lag.\n",
            ),
            raw(
                "runbooks/cache.md",
                "sha-b",
                "---\ntype: runbook\ntopic: Cache Flush\ntags: [ops]\n---\nFlush cache state after deploy.\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-search")
        .await
        .unwrap();

    let hits = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "replica drift".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "concepts/replica.md");
    assert!(hits[0].snippet.contains("[replica]") || hits[0].snippet.contains("[Replica]"));
    assert!(hits[0].score > 0.0);

    let tag_hits = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "priority".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(tag_hits[0].path, "concepts/replica.md");
}

#[tokio::test]
async fn search_finds_path_prefix_and_prefers_title_matches() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-search-pokemon", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "pokemon/025-pikachu.md",
                "sha-a",
                "---\ntype: concept\ntopic: Electric Mouse\ntags: [electric]\n---\nStatic ability.\n",
            ),
            raw(
                "pokemon/026-raichu.md",
                "sha-b",
                "---\ntype: concept\ntopic: Raichu\ntags: [electric]\n---\nPikachu evolves with a thunder stone.\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-search-pokemon")
        .await
        .unwrap();

    let slug_hits = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "pika".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(slug_hits[0].path, "pokemon/025-pikachu.md");

    let title_hits = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "raichu".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(title_hits[0].path, "pokemon/026-raichu.md");
}

#[tokio::test]
async fn search_applies_structured_filters() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-search-filters", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "concepts/alpha.md",
                "sha-a",
                "---\ntype: concept\ntopic: Alpha\ntags: [sync]\n---\nShared recovery checklist.\n",
            ),
            raw(
                "runbooks/beta.md",
                "sha-b",
                "---\ntype: runbook\ntopic: Beta\ntags: [ops]\n---\nShared recovery checklist.\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-search-filters")
        .await
        .unwrap();

    let hits = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "recovery checklist".to_string(),
            node_types: vec!["runbook".to_string()],
            path_prefix: Some("runbooks".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "runbooks/beta.md");

    let tagged = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "recovery checklist".to_string(),
            tags: vec!["SYNC".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(tagged.len(), 1);
    assert_eq!(tagged[0].path, "concepts/alpha.md");
}

#[tokio::test]
async fn search_rebuild_removes_stale_rows() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let target = target("org", "repo-search-stale", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let first = ProjectionSnapshot::from_raw_files(
        &[raw(
            "concepts/old.md",
            "sha-a",
            "---\ntype: concept\ntopic: Old\n---\nstale-only phrase\n",
        )],
        &config,
    );
    persist_snapshot(&pool, target_id, &first, "first")
        .await
        .unwrap();

    let second = ProjectionSnapshot::from_raw_files(
        &[raw(
            "concepts/new.md",
            "sha-b",
            "---\ntype: concept\ntopic: New\n---\nfresh-only phrase\n",
        )],
        &config,
    );
    persist_snapshot(&pool, target_id, &second, "second")
        .await
        .unwrap();

    let stale = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "stale-only".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let fresh = search_nodes_from_pool(
        &pool,
        target_id,
        &SearchFilters {
            q: "fresh-only".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(stale.is_empty());
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].path, "concepts/new.md");
}

#[tokio::test]
async fn list_work_items_filters_projection_rows() {
    let pool = test_pool().await;
    let config = BrainConfig::parse(
        r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#fb7185"
    title_key: topic
    work_item_kind: task
"##,
    )
    .unwrap();

    let target = target("org", "repo-workitem-query", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[
            raw(
                "tasks/a.md",
                "sha-a",
                "---\ntype: task\ntopic: A\nbrain_id: task-a\nstate: blocked\nassignees: [andrea]\n---\n",
            ),
            raw(
                "tasks/b.md",
                "sha-b",
                "---\ntype: task\ntopic: B\nbrain_id: task-b\nstate: done\nassignees: [sam]\n---\n",
            ),
        ],
        &config,
    );
    persist_snapshot(&pool, target_id, &snapshot, "test-workitem-query")
        .await
        .unwrap();

    let blocked = list_work_items_from_pool(
        &pool,
        target_id,
        &WorkItemFilters {
            states: vec![WorkItemState::Blocked],
            assignees: vec!["ANDREA".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].brain_id, "task-a");
}

#[tokio::test]
async fn migrate_adds_registration_columns_to_targets() {
    let pool = test_pool().await;
    let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('targets')")
        .fetch_all(&pool)
        .await
        .unwrap();
    let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
    for required in ["registered_at", "registered_by", "source", "default_branch"] {
        assert!(
            names.contains(&required),
            "targets is missing column {required}; got {names:?}"
        );
    }
}

#[tokio::test]
async fn migrate_creates_unique_index_on_org_repo() {
    let pool = test_pool().await;
    // The UNIQUE index must reject a second row with the same (org, repo)
    // even when the branch differs - this is the stickiness invariant.
    let _ = ensure_target_id(&pool, &target("acme", "kb", "main"))
        .await
        .unwrap();
    let res = sqlx::query("INSERT INTO targets (key, org, repo, branch) VALUES (?, ?, ?, ?)")
        .bind("acme/kb/develop")
        .bind("acme")
        .bind("kb")
        .bind("develop")
        .execute(&pool)
        .await;
    assert!(res.is_err(), "expected UNIQUE(org, repo) violation, got Ok");
}

#[tokio::test]
async fn migrate_is_idempotent() {
    // Running migrate repeatedly on the same pool must not fail. Sqlx should
    // skip recorded migrations, and the legacy-target preflight must stay
    // idempotent.
    let pool = test_pool().await;
    migrate(&pool).await.expect("second migrate must succeed");
    migrate(&pool).await.expect("third migrate must succeed");
}

#[tokio::test]
async fn ensure_target_id_seeds_registration_metadata() {
    // Default seed for rows created via the existing ensure_target_id
    // path (which doesn't yet know about the new columns) must be the
    // schema default `'env_default'` and CURRENT_TIMESTAMP.
    let pool = test_pool().await;
    let _id = ensure_target_id(&pool, &target("acme", "kb", "main"))
        .await
        .unwrap();
    let row: (String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT source, registered_by, registered_at, default_branch FROM targets
         WHERE org = ? AND repo = ?",
    )
    .bind("acme")
    .bind("kb")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "env_default");
    assert!(row.1.is_none());
    assert!(!row.2.is_empty());
    assert_eq!(row.3.as_deref(), Some("main"));
}

#[tokio::test]
async fn pending_sync_enqueue_dedupes_and_bumps_attempts() {
    let pool = test_pool().await;
    let t = target("Org", "Repo", "main");
    let target_id = ensure_target_id(&pool, &t).await.unwrap();

    // First failure inserts a row at attempt 1.
    pending_sync::enqueue(&pool, target_id, "wi-1", "state", "boom")
        .await
        .unwrap();
    // Same (target, brain_id, kind) failing again bumps attempts, no dup row.
    pending_sync::enqueue(&pool, target_id, "wi-1", "state", "boom again")
        .await
        .unwrap();
    // A different kind for the same item is a distinct row.
    pending_sync::enqueue(&pool, target_id, "wi-1", "assignees", "nope")
        .await
        .unwrap();

    let rows = pending_sync::list_all(&pool, 100).await.unwrap();
    assert_eq!(rows.len(), 2, "state row deduped, assignees row distinct");
    let state_row = rows.iter().find(|r| r.kind == "state").unwrap();
    assert_eq!(state_row.attempts, 2);
    assert_eq!(state_row.last_error.as_deref(), Some("boom again"));
    assert_eq!(state_row.org, "Org");
    assert_eq!(state_row.repo, "Repo");

    // Retry failure bumps without inserting.
    pending_sync::record_retry_failure(&pool, state_row.id, "still failing")
        .await
        .unwrap();
    let after = pending_sync::list_all(&pool, 100).await.unwrap();
    let state_after = after.iter().find(|r| r.kind == "state").unwrap();
    assert_eq!(state_after.attempts, 3);

    // Delete clears the row (success path).
    pending_sync::delete(&pool, state_after.id).await.unwrap();
    let remaining = pending_sync::list_all(&pool, 100).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].kind, "assignees");

    // next_batch surfaces the remaining job with its identity + kind for retry.
    let batch = pending_sync::next_batch(&pool, 10, 20).await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].brain_id, "wi-1");
    assert_eq!(batch[0].branch, "main");
    assert_eq!(batch[0].kind, "assignees");
}

#[tokio::test]
async fn pending_sync_next_batch_skips_exhausted_rows() {
    let pool = test_pool().await;
    let t = target("Org", "Repo", "main");
    let target_id = ensure_target_id(&pool, &t).await.unwrap();

    pending_sync::enqueue(&pool, target_id, "wi-exhausted", "state", "permanent")
        .await
        .unwrap();
    pending_sync::enqueue(&pool, target_id, "wi-ready", "state", "transient")
        .await
        .unwrap();

    let exhausted = pending_sync::list_all(&pool, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.brain_id == "wi-exhausted")
        .unwrap();
    for _ in 0..19 {
        pending_sync::record_retry_failure(&pool, exhausted.id, "still permanent")
            .await
            .unwrap();
    }

    let batch = pending_sync::next_batch(&pool, 10, 20).await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].brain_id, "wi-ready");
}

#[tokio::test]
async fn migrate_records_schema_version_on_fresh_db() {
    let pool = test_pool().await;
    let version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 6, "expected all projection migrations applied");
}

#[tokio::test]
async fn migrate_claims_baseline_on_legacy_db() {
    // Simulate a prod DB that already has the pre-versioned schema (every
    // table created by the old ad-hoc `migrate()`). The new sqlx migrator
    // must see the existing tables, treat 0001 as a no-op (every statement
    // is idempotent), record the current migration chain in `_sqlx_migrations`,
    // and apply the projection column additions cleanly.
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();

    // Inline copy of the pre-slice DDL (subset that would already exist on a
    // legacy DB — every CREATE in baseline uses IF NOT EXISTS so this is a
    // realistic precondition).
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            org TEXT NOT NULL,
            repo TEXT NOT NULL,
            branch TEXT NOT NULL,
            registered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            registered_by TEXT,
            source TEXT NOT NULL DEFAULT 'env_default',
            default_branch TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS files (
            target_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            sha TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (target_id, path),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS nodes (
            target_id INTEGER NOT NULL,
            node_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            node_type TEXT NOT NULL,
            tags_json TEXT NOT NULL,
            x REAL NOT NULL,
            y REAL NOT NULL,
            path TEXT NOT NULL,
            sha TEXT NOT NULL,
            is_virtual INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (target_id, node_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projection_sync_state (
            target_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'stale',
            last_attempt_at TEXT,
            last_success_at TEXT,
            last_error_at TEXT,
            last_error TEXT,
            last_reason TEXT,
            file_count INTEGER NOT NULL DEFAULT 0,
            node_count INTEGER NOT NULL DEFAULT 0,
            edge_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO targets (key, org, repo, branch) VALUES ('o/r/main', 'o', 'r', 'main')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Now run the new migrator. This must not error even though tables exist.
    migrate(&pool).await.unwrap();

    // Verify 0001 + current migrations are recorded.
    let version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        version, 6,
        "legacy DB should claim baseline + apply current migrations"
    );

    // Verify projection extension columns added.
    let files_cols: Vec<(i64, String)> =
        sqlx::query_as("SELECT cid, name FROM pragma_table_info('files')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let files_col_names: Vec<&str> = files_cols.iter().map(|(_, n)| n.as_str()).collect();
    assert!(files_col_names.contains(&"body_text"));
    assert!(files_col_names.contains(&"frontmatter_json"));

    let edge_cols: Vec<(i64, String)> =
        sqlx::query_as("SELECT cid, name FROM pragma_table_info('edges')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let edge_col_names: Vec<&str> = edge_cols.iter().map(|(_, n)| n.as_str()).collect();
    assert!(edge_col_names.contains(&"kind"));
    assert!(files_col_names.contains(&"blob_sha"));

    // node_authors table created.
    let authors_cols: Vec<(i64, String)> =
        sqlx::query_as("SELECT cid, name FROM pragma_table_info('node_authors')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!authors_cols.is_empty(), "node_authors table must exist");

    // Existing data preserved.
    let target_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM targets")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(target_count, 1, "pre-existing target row should survive");
}

#[tokio::test]
async fn migrate_heals_targets_table_missing_registration_columns() {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();

    sqlx::query(
        "CREATE TABLE targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            org TEXT NOT NULL,
            repo TEXT NOT NULL,
            branch TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO targets (key, org, repo, branch) VALUES ('o/r/main', 'o', 'r', 'main')",
    )
    .execute(&pool)
    .await
    .unwrap();

    migrate(&pool).await.unwrap();

    let row: (String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT source, registered_by, registered_at, default_branch FROM targets
         WHERE org = 'o' AND repo = 'r'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.0, "env_default");
    assert!(row.1.is_none());
    assert!(!row.2.is_empty());
    assert_eq!(row.3.as_deref(), Some("main"));
}

#[tokio::test]
async fn snapshot_populates_body_frontmatter_and_authors() {
    let pool = test_pool().await;
    let config = BrainConfig::default();

    let content = "---\ntype: concept\ntopic: A\nauthors:\n  - alice\n  - bob\n---\n# Body line\n";
    let snapshot =
        ProjectionSnapshot::from_raw_files(&[raw("concepts/A.md", "sha-a", content)], &config);

    let target = target("org", "repo", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    persist_snapshot(&pool, target_id, &snapshot, "test")
        .await
        .unwrap();

    let (file_body, file_front, file_blob_sha): (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT body_text, frontmatter_json, blob_sha FROM files WHERE target_id = ?",
        )
        .bind(target_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(file_body.as_deref().unwrap_or("").contains("Body line"));
    assert_eq!(file_blob_sha.as_deref(), Some("sha-a"));
    let front_json = file_front.expect("frontmatter_json populated");
    assert!(front_json.contains("\"alice\""));

    let (node_body, node_front, node_blob_sha): (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT body_text, frontmatter_json, blob_sha FROM nodes WHERE target_id = ?",
        )
        .bind(target_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(node_body.as_deref().unwrap_or("").contains("Body line"));
    assert!(node_front.is_some());
    assert_eq!(node_blob_sha.as_deref(), Some("sha-a"));

    let authors: Vec<(String, String)> =
        sqlx::query_as("SELECT author, role FROM node_authors WHERE target_id = ? ORDER BY author")
            .bind(target_id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0].0, "alice");
    assert_eq!(authors[0].1, "author");
    assert_eq!(authors[1].0, "bob");

    // last_rebuild_duration_ms is set by `rebuild` (post-write UPDATE),
    // not by `persist_snapshot`. After persist alone the column is NULL —
    // confirming nothing else is sneaking a value in.
    let duration: Option<i64> = sqlx::query_scalar(
        "SELECT last_rebuild_duration_ms FROM projection_sync_state WHERE target_id = ?",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(duration, None);
}

async fn stub_tower_sessions(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tower_sessions (
            id TEXT PRIMARY KEY,
            data BLOB NOT NULL,
            expiry_date INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn retention_deletes_old_audit_events() {
    let pool = test_pool().await;
    stub_tower_sessions(&pool).await;

    for _ in 0..5 {
        sqlx::query("INSERT INTO audit_events (ts, kind, detail) VALUES (datetime('now', '-100 days'), 'old', 'x')")
            .execute(&pool)
            .await
            .unwrap();
    }
    for _ in 0..5 {
        sqlx::query("INSERT INTO audit_events (kind, detail) VALUES ('new', 'y')")
            .execute(&pool)
            .await
            .unwrap();
    }

    crate::server::retention::run_once(&pool, 90).await.unwrap();

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 5, "old rows should be deleted, new rows kept");
}

#[tokio::test]
async fn retention_deletes_expired_sessions() {
    let pool = test_pool().await;
    stub_tower_sessions(&pool).await;

    sqlx::query(
        "INSERT INTO tower_sessions (id, data, expiry_date)
         VALUES ('past_text', x'00', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour'))",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tower_sessions (id, data, expiry_date)
         VALUES ('future_text', x'00', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+7 days'))",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tower_sessions (id, data, expiry_date)
         VALUES ('past_epoch', x'00', CAST(strftime('%s', 'now', '-1 day') AS INTEGER))",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tower_sessions (id, data, expiry_date)
         VALUES ('future_epoch', x'00', CAST(strftime('%s', 'now', '+7 days') AS INTEGER))",
    )
    .execute(&pool)
    .await
    .unwrap();

    crate::server::retention::run_once(&pool, 90).await.unwrap();

    let remaining: Vec<(String,)> = sqlx::query_as("SELECT id FROM tower_sessions ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let remaining_ids: Vec<String> = remaining.into_iter().map(|(id,)| id).collect();
    assert_eq!(remaining_ids, vec!["future_epoch", "future_text"]);
}

/// Operator smoke test: run the migrator against a copy of a real prod
/// SQLite file. Ignored by default; invoke with:
///
/// ```text
/// LEGACY_DB_SMOKE=/tmp/sessions-legacy.db \
///     cargo test --features ssr -p gitnodes-app \
///     -- --ignored legacy_db_smoke
/// ```
///
/// The file should be a *copy* of a real `data/sessions.db`; the test
/// mutates it. Verifies migrations claim baseline + add v2 columns without
/// corrupting existing rows.
#[tokio::test]
#[ignore]
async fn legacy_db_smoke() {
    let path = match std::env::var("LEGACY_DB_SMOKE") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!("LEGACY_DB_SMOKE not set — skipping");
            return;
        }
    };
    let url = format!("sqlite://{path}");
    let opts = SqliteConnectOptions::from_str(&url)
        .unwrap()
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("open legacy DB");

    let before_targets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM targets")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);

    migrate(&pool).await.expect("migrate legacy DB");

    let version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        version, 6,
        "legacy DB should be at current schema after migrate"
    );

    let files_cols: Vec<(i64, String)> =
        sqlx::query_as("SELECT cid, name FROM pragma_table_info('files')")
            .fetch_all(&pool)
            .await
            .unwrap();
    let names: Vec<&str> = files_cols.iter().map(|(_, n)| n.as_str()).collect();
    assert!(names.contains(&"body_text"), "files.body_text must exist");
    assert!(names.contains(&"frontmatter_json"));
    assert!(names.contains(&"blob_sha"));

    let after_targets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM targets")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before_targets, after_targets,
        "target rows must not be lost"
    );

    eprintln!(
        "legacy_db_smoke OK — path={path}, schema_version={version}, targets={after_targets}"
    );
}

#[tokio::test]
async fn projection_status_reports_schema_and_per_target_state() {
    let pool = test_pool().await;
    let config = BrainConfig::default();
    let snapshot = ProjectionSnapshot::from_raw_files(
        &[raw(
            "concepts/A.md",
            "sha-a",
            "---\ntype: concept\ntopic: A\n---\nbody\n",
        )],
        &config,
    );
    let target = target("org", "repo", "main");
    let target_id = ensure_target_id(&pool, &target).await.unwrap();
    persist_snapshot(&pool, target_id, &snapshot, "test")
        .await
        .unwrap();
    // Simulate the post-write UPDATE that `rebuild` performs after timing
    // the full fetch+parse+write cycle.
    sqlx::query(
        "UPDATE projection_sync_state SET last_rebuild_duration_ms = 17 WHERE target_id = ?",
    )
    .bind(target_id)
    .execute(&pool)
    .await
    .unwrap();

    let version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 6);

    let row: (String, String, i64, i64, Option<i64>) = sqlx::query_as(
        "SELECT t.org, COALESCE(s.status, 'stale'), COALESCE(s.file_count, 0), COALESCE(s.node_count, 0), s.last_rebuild_duration_ms
         FROM targets t LEFT JOIN projection_sync_state s ON s.target_id = t.id
         WHERE t.id = ?",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "org");
    assert_eq!(row.1, "ready");
    assert_eq!(row.2, 1);
    assert_eq!(row.3, 1);
    assert_eq!(row.4, Some(17));
}
