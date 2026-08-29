# Architecture

GitNodes is a full-stack Rust application over a deliberately simple content
model: a Git repository of markdown files with YAML frontmatter. Git is the
source of truth. Every database table, graph edge, search result, and UI view is
derived from that repository or is operational state such as a session or audit
record.

This guide explains the major components, data flows, runtime modes, and trust
boundaries. For user-facing behavior, see the [feature inventory](FEATURES.md).
For contributor commands and code conventions, see
[`CONTRIBUTING.md`](../CONTRIBUTING.md) and [`AGENTS.md`](../AGENTS.md).

## System shape

```text
                         GitHub repository
                      markdown + .gitnodes.yml
                         ▲                │
                         │ writes         │ raw files / webhooks
                         │                ▼
 browser ──HTTP──▶ Leptos/Axum app ──▶ projection rebuild
    ▲                    │                      │
    │ SSR + hydrate      │                      ▼
    └────────────────────┘              target-scoped SQLite
                                          graph + FTS5

 local checkout ──▶ preview server ──▶ in-memory projection ──▶ browser
        │
        └──────────▶ MCP server ──stdio──▶ coding agent
             read working tree       agent edits files through Git
```

The browser never writes SQLite as a substitute for Git. A mutation creates a
commit—or a pull request—and the projection updates only after rebuilding from
the repository state. This avoids a dual-write protocol and makes recovery
mechanical: fetch the files again and rebuild.

## Runtime modes

The same binary exposes several modes with intentionally different source and
security boundaries.

| Mode | Content source | Projection | Authentication | Writes |
|---|---|---|---|---|
| `gitnodes preview [dir]` | Local working tree | In-memory SQLite | None; loopback by default | Disabled |
| `gitnodes mcp [dir]` | Local working tree | In-memory SQLite | Stdio process boundary | Read-only tools |
| `gitnodes serve [dir]` | Pushed GitHub branch | Persistent SQLite | Existing `gh auth`/PAT | Permission-aware GitHub writes |
| Hosted server | Configured GitHub targets | Persistent SQLite | OAuth, PAT, or GitHub App support | Permission-aware GitHub writes |

`preview` and `mcp` see uncommitted files. `serve` and hosted deployments see
the configured remote branch. That boundary prevents a local working tree from
silently becoming shared application state.

## Workspace and dependency direction

The Cargo workspace contains five crates. Dependencies point inward toward the
pure domain layer:

```text
gitnodes-app ──────▶ gitnodes-storage ─┐
      │                                │
      ├────────────▶ gitnodes-auth ────┼──▶ gitnodes-domain
      │                                │
      └────────────▶ gitnodes-graph ───┘
```

### `gitnodes-domain`

Pure, WASM-safe types and parsing helpers:

- `BrainConfig`, node type and saved-view specifications;
- `Node`, `Edge`, targets, and work-item domain values;
- frontmatter splitting and domain validation;
- no network, database, or filesystem I/O.

### `gitnodes-graph`

Pure graph construction and layout:

- parse markdown body links;
- resolve configured frontmatter relationships;
- create shared-tag edges;
- assign stable graph positions through force-directed layout.

Keeping this crate I/O-free lets server rendering and hydrated WASM share the
same graph semantics.

### `gitnodes-auth`

GitHub authentication primitives:

- OAuth state and token exchange;
- optional organization-membership gates;
- encrypted session token handling.

Repository authorization does not live here as a one-time login claim. The app
checks live repository permissions per request, with a short cache.

### `gitnodes-storage`

GitHub transport and mutation machinery:

- repository trees, blobs, files, branches, and pull requests;
- the atomic multi-file Git Data API transaction layer;
- SHA preconditions, conflict handling, retry with jitter, and cleanup of
  temporary PR branches.

This crate knows how to make a GitHub change safely. It does not update the
projection.

### `gitnodes-app`

The Leptos 0.8 SSR/WASM application and Axum server:

- HTML server rendering and browser hydration;
- CLI subcommands and local modes;
- server functions and GitHub write orchestration;
- sessions, audit records, projection storage, webhooks, and SSE;
- knowledge graph, search, editor, administration, and MCP surfaces.

Server-only code is gated by the `ssr` feature. Browser-only code is gated by
`hydrate`; dependencies such as Tokio, reqwest, and SQLx must never leak into
the WASM build.

## Content model

A Brain is a repository containing:

- markdown notes;
- optional YAML frontmatter on each note;
- `.gitnodes.yml`, defining types, directories, typed fields, views, and work
  items;
- optional templates and assets;
- generated `AGENTS.md` and client configuration teaching coding agents the
  same taxonomy.

Three edge families are projected:

| Edge family | Source | Example |
|---|---|---|
| Body | Relative markdown link | A postmortem links to an incident |
| Frontmatter | A field declared in `link_fields` | `supersedes: 0004...` |
| Tag | Shared tag membership | Two notes tagged `outage-2026-03-11` |

Typed frontmatter edges are directional. That matters when traversing history:
ADR-0009 points to ADR-0004 through `supersedes`, so a reader starting from the
old ADR must inspect incoming relationships to learn that it is no longer
current.

## Projection lifecycle

The persistent projection is target-scoped. A target is an owner, repository,
and branch; data from two branches never shares a graph namespace.

A rebuild performs these steps:

1. Resolve and authorize the target.
2. Fetch the raw repository files and the target `.gitnodes.yml`.
3. Parse notes and build the graph in memory.
4. Start a SQLite transaction.
5. Replace target-scoped nodes, edges, files, backlinks, work items, and search
   rows as one snapshot.
6. Record the source watermark and mark the projection fresh.
7. Publish a target-scoped SSE event to connected clients.

Blob SHAs help detect drift. A projection failure leaves the previous snapshot
available but stale; it does not create a partly updated graph.

Search combines SQLite FTS5 ranking with structured matches such as type, tag,
and path using reciprocal-rank fusion. The browser and MCP server use the same
projection and ranking path.

Local modes build an in-memory projection instead. MCP fingerprints the working
tree with file size and modification time, then debounces rebuilds so repeated
tool calls are cheap while fresh edits remain visible.

## Write path

All web mutations pass through the permission-aware write orchestrator:

```text
user intent
    │
    ├── live repository permission check
    │
    ├── push allowed ─────────────▶ atomic commit
    │
    └── review required/requested ─▶ temporary branch ─▶ pull request
                                                   │
                                                   └── cleanup on failure
```

Operations that touch several files—particularly rename plus backlink
rewrites—use `GitTransaction`. It verifies expected SHAs and produces one commit
or no commit. Concurrent changes surface as typed conflicts; GitNodes does not
attempt an unsafe semantic merge.

After a successful write, GitHub remains authoritative. Webhook or explicit
refresh causes a projection rebuild. The application never patches a projection
row merely because it expects the upstream write to succeed.

## Work-item synchronization

Some node types can represent operational work such as tasks, incidents, or
decisions. A work item may bind to a GitHub issue or pull request and declare
which side is authoritative:

- `brain`: markdown wins;
- `external`: the provider wins for synchronized fields;
- `split`: GitNodes coordinates fields in both directions.

Provider updates are side effects after the Git content mutation. Failed
provider pushes enter a supervised retry outbox rather than rolling back an
already valid Git commit. The domain model anticipates other forges, but the
runtime provider adapter is currently GitHub-specific.

## Routing and multi-target isolation

Canonical routes contain four target segments:

```text
/{owner}/{repository}/{branch}/knowledge
```

Legacy three-segment routes resolve through `target_registry`. A registry row is
persisted only after a repository has been confirmed as an accessible Brain;
repository listings alone never create routing state.

Configuration caches, graph snapshots, searches, authorization checks, SSE
channels, audit events, and work-item operations all carry the resolved target.

## Security boundaries

- **Authentication:** OAuth, deliberate single-user PAT mode, or the local
  GitHub CLI credential path.
- **Authorization:** live GitHub `repository_permissions` checks; an optional
  login organization is an additional gate, never a replacement.
- **Sessions:** encrypted tokens in persistent SQLite, backed by a durable
  encryption key.
- **Mutations:** CSRF origin checks, rate limiting, permission checks, and SHA
  preconditions.
- **Webhooks:** HMAC-SHA256 verification before background work is scheduled.
- **Markdown:** raw HTML is escaped and rendered output is sanitized.
- **Private assets:** fetched server-side through an authenticated proxy; user
  tokens are never exposed to the browser.
- **Local modes:** bind to loopback unless a deliberate escape hatch is set.

For deploy-time configuration and exact environment variables, see
[Deployment](guides/DEPLOYMENT.md). For limitations and recovery procedures,
see [Operator notes](OPERATOR_NOTES.md).

## Frontend architecture

The server renders the first page through Leptos, then the WASM bundle hydrates
interactive behavior. Major knowledge UI components are separated by concern:

- `knowledge/page.rs`: route composition and shared state;
- `knowledge/graph_canvas.rs`: SVG graph interaction, camera, selection, and
  typed-edge rendering;
- `knowledge/filter_panel.rs`: URL-backed type, tag, path, and saved-view state;
- `knowledge/detail_panel.rs`: rendered note, backlinks, and work-item context;
- `knowledge/editor/`: create and edit flows, frontmatter, relationships,
  markdown preview, and draft persistence;
- `knowledge/live_sync.rs`: SSE freshness signals;
- `admin/`: configuration, sessions, audit, and projection operations.

Tailwind scans Rust sources for class names. CSS compilation is a separate Node
pipeline, while the runtime application remains Rust/WASM.

## Build forms

Development uses separate server, WASM, and CSS outputs under `target/site`.
Release builds may enable `embed-assets`, which compiles that site directory
into the executable and extracts it once into a versioned cache. Distribution
therefore remains a single binary without changing the request or asset model.

The CI matrix independently checks:

- formatting;
- server-side clippy and tests;
- hydrated WASM clippy for `wasm32-unknown-unknown`;
- all non-app workspace crates;
- release-sensitive embedded-asset and dependency checks.

## Design invariants

Changes should preserve these properties:

1. Git remains the only content source of truth.
2. The projection rebuilds from a clean clone alone.
3. A repository mutation never directly updates projection content.
4. Target identity accompanies every cached or persisted content record.
5. Server-only dependencies stay out of the hydrate build.
6. Agents discover through read-only MCP and author through ordinary Git.
7. Known lossy frontmatter formatting is not “fixed” inside an unrelated write
   path; preserving YAML syntax requires a dedicated round-trip design.
