use std::str::FromStr;

use brain_domain::{BrainConfig, TargetConfig, WorkItemState, WorkItemSystemOfRecord};
use brain_graph::RawFile;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use super::{
    FileFilters, NodeFilters, WorkItemFilters,
    files::list_files_from_pool,
    migrations::migrate,
    nodes::{list_nodes_from_pool, load_cached_graph},
    rebuild::{ProjectionSnapshot, persist_snapshot},
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
            "---\ntype: task\ntopic: API read\nbrain_id: task-api-1\nstate: done\nassignees: [andrea]\nexternal_binding:\n  system: github\n  project: AndreaBozzo/Brain_UI\n  item_key: \"77\"\n  provider_id: I_kwDO123\n  url: https://github.com/AndreaBozzo/Brain_UI/issues/77\nsystem_of_record: split\n---\n# Task: API read\n",
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
    // Running migrate twice on the same pool must not fail. ALTER TABLE
    // ADD COLUMN would error on the second run without the
    // `add_column_if_missing` guard.
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
