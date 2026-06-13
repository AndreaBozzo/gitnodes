use brain_domain::{GithubClient, TargetConfig};
use brain_storage::{
    BackoffPolicy, BranchTransaction, GitTransaction, GithubHttp, PreconditionStatus,
};
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer, branch: &str) -> GithubClient {
    GithubClient::new(TargetConfig {
        org: "acme".into(),
        repo: "kb".into(),
        branch: branch.into(),
    })
    .with_api_base(server.uri())
}

async fn mount_plan_reads(server: &MockServer, entries: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/refs/heads/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": { "sha": "HEAD0" }
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/commits/HEAD0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": { "sha": "TREE0" }
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/trees/TREE0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": entries,
            "truncated": false
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn plan_is_read_only_and_reports_satisfied_preconditions() {
    let server = MockServer::start().await;
    mount_plan_reads(
        &server,
        json!([{ "path": "notes/a.md", "type": "blob", "sha": "OLD" }]),
    )
    .await;
    for verb in ["POST", "PATCH", "DELETE"] {
        Mock::given(method(verb))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
    }

    let transaction = GitTransaction::new("update", "alice", "alice@example.com")
        .upsert_text("notes/a.md", "new")
        .expect_sha("notes/a.md", "OLD")
        .expect_absent("notes/new.md");
    let plan = transaction
        .plan(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect("plan");

    assert!(plan.can_commit());
    assert_eq!(plan.head_sha, "HEAD0");
    assert_eq!(plan.base_tree_sha, "TREE0");
    assert_eq!(plan.upserts[0].path, "notes/a.md");
    assert_eq!(plan.upserts[0].byte_len, 3);
    assert!(
        plan.preconditions
            .iter()
            .all(|check| check.status == PreconditionStatus::Satisfied)
    );
}

#[tokio::test]
async fn plan_returns_failed_precondition_without_mutating() {
    let server = MockServer::start().await;
    mount_plan_reads(
        &server,
        json!([{ "path": "notes/new.md", "type": "blob", "sha": "TAKEN" }]),
    )
    .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let transaction = GitTransaction::new("create", "alice", "alice@example.com")
        .upsert_text("notes/new.md", "new")
        .expect_absent("notes/new.md");
    let plan = transaction
        .plan(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect("plan");

    assert!(!plan.can_commit());
    assert!(matches!(
        plan.preconditions[0].status,
        PreconditionStatus::Failed { .. }
    ));
}

#[tokio::test]
async fn plan_rejects_truncated_recursive_tree() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/refs/heads/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": { "sha": "HEAD0" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/commits/HEAD0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": { "sha": "TREE0" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/kb/git/trees/TREE0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": [],
            "truncated": true
        })))
        .mount(&server)
        .await;

    let error = GitTransaction::new("create", "alice", "alice@example.com")
        .upsert_text("new.md", "new")
        .expect_absent("new.md")
        .plan(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect_err("truncated plan");
    assert!(error.to_string().contains("truncated"));
}

#[tokio::test]
async fn branch_creation_retries_fork_propagation_then_commits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/refs"))
        .respond_with(ResponseTemplate::new(404).set_body_string("fork not ready"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/refs"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    mount_commit_pipeline(&server, "patch/alice/change", "BASE", "TREE0", "COMMIT1").await;

    let outcome = BranchTransaction::new("BASE", "patch/alice/change")
        .with_branch_policy(BackoffPolicy {
            max_attempts: 4,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        })
        .add(GitTransaction::new("change", "alice", "alice@example.com").upsert_text("a.md", "a"))
        .commit_all(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect("branch transaction");

    assert_eq!(outcome.branch, "patch/alice/change");
    assert_eq!(outcome.head_sha, "COMMIT1");
    assert_eq!(outcome.commits.len(), 1);
}

#[tokio::test]
async fn successful_branch_can_be_rolled_back_after_later_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/refs"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    mount_commit_pipeline(&server, "patch/alice/change", "BASE", "TREE0", "COMMIT1").await;
    Mock::given(method("DELETE"))
        .and(path("/repos/acme/kb/git/refs/heads/patch/alice/change"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let http = GithubHttp::new().unwrap();
    let gh = client(&server, "main");
    let outcome = BranchTransaction::new("BASE", "patch/alice/change")
        .add(GitTransaction::new("change", "alice", "alice@example.com").upsert_text("a.md", "a"))
        .commit_all(&http, &gh, "tok")
        .await
        .expect("branch transaction");

    outcome
        .rollback(&http, &gh, "tok")
        .await
        .expect("rollback after PR failure");
}

#[tokio::test]
async fn branch_transaction_rolls_back_when_commit_fails() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/refs"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/blobs"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/repos/acme/kb/git/refs/heads/patch/alice/change"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let error = BranchTransaction::new("BASE", "patch/alice/change")
        .add(GitTransaction::new("change", "alice", "alice@example.com").upsert_text("a.md", "a"))
        .commit_all(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect_err("commit should fail");
    assert!(error.to_string().contains("500"));
}

#[tokio::test]
async fn branch_transaction_chains_multiple_commits() {
    let server = MockServer::start().await;
    let branch = "patch/alice/batch";
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/refs"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/blobs"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "BLOB" })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/kb/git/refs/heads/{branch}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": { "sha": "BASE" }
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/kb/git/refs/heads/{branch}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": { "sha": "COMMIT1" }
        })))
        .mount(&server)
        .await;
    for (head, tree) in [("BASE", "TREE0"), ("COMMIT1", "TREE1")] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/acme/kb/git/commits/{head}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": tree }
            })))
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/trees"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "NEXT_TREE" })))
        .mount(&server)
        .await;
    for (parent, commit) in [("BASE", "COMMIT1"), ("COMMIT1", "COMMIT2")] {
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/commits"))
            .and(body_partial_json(json!({ "parents": [parent] })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": commit })))
            .mount(&server)
            .await;
    }
    Mock::given(method("PATCH"))
        .and(path(format!("/repos/acme/kb/git/refs/heads/{branch}")))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let outcome = BranchTransaction::new("BASE", branch)
        .add(GitTransaction::new("one", "alice", "alice@example.com").upsert_text("a.md", "a"))
        .add(GitTransaction::new("two", "alice", "alice@example.com").upsert_text("b.md", "b"))
        .commit_all(&GithubHttp::new().unwrap(), &client(&server, "main"), "tok")
        .await
        .expect("batch");

    assert_eq!(outcome.commits.len(), 2);
    assert_eq!(outcome.commits[1].head_before, "COMMIT1");
    assert_eq!(outcome.head_sha, "COMMIT2");
}

async fn mount_commit_pipeline(
    server: &MockServer,
    branch: &str,
    head: &str,
    tree: &str,
    commit: &str,
) {
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/blobs"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "BLOB" })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/kb/git/refs/heads/{branch}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": { "sha": head }
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/kb/git/commits/{head}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": { "sha": tree }
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/trees"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "TREE1" })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/kb/git/commits"))
        .and(body_partial_json(json!({ "parents": [head] })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": commit })))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/repos/acme/kb/git/refs/heads/{branch}")))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}
