# Getting started

This guide takes one knowledge base from a local, read-only preview to
GitHub-backed editing. No GitHub account or credential is needed until
[Publish and serve it](#publish-and-serve-it).

## Installation status

GitNodes is currently pre-release. The installer scripts and Homebrew/WinGet
metadata are prepared for the public upstream, but they are not a usable
installation path until that repository publishes its first release.

For now, build from this checkout:

```bash
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --locked --version 0.3.6
npm ci
npm run build:css
cargo leptos build --release
cargo build --release -p gitnodes-app --bin gitnodes-app \
  --no-default-features --features embed-assets
```

The self-contained executable is `target/release/gitnodes-app` on macOS/Linux
and `target/release/gitnodes-app.exe` on Windows. Put it on `PATH` as
`gitnodes`/`gitnodes.exe`.

## Create and preview a brain

```bash
gitnodes init my-brain
cd my-brain
gitnodes preview
```

`init` creates:

- `.gitnodes.yml`, with starter node types and a saved view;
- linked example notes;
- `AGENTS.md`, generated from the configured taxonomy;
- `.gitignore` entries for local secrets and runtime data;
- a local Git repository when Git is available.

`preview` opens a loopback-only, read-only web UI. It uses an in-memory SQLite
projection, requires no login, and writes no runtime state into the brain.
Edit markdown in an editor and refresh the graph to see the working-tree state.

## Connect an agent

Do not normally launch `gitnodes mcp` by hand. Configure the agent client to
launch it as a stdio subprocess:

```bash
claude mcp add gitnodes -- gitnodes mcp /absolute/path/to/my-brain
codex mcp add gitnodes -- gitnodes mcp /absolute/path/to/my-brain
```

JSON-based clients use the same command:

```json
{
  "mcpServers": {
    "gitnodes": {
      "command": "gitnodes",
      "args": ["mcp", "/absolute/path/to/my-brain"]
    }
  }
}
```

The MCP server exposes `search_brain`, `list_nodes`, `read_node`, and
`node_links`. It is deliberately read-only. Agents edit markdown in the
checkout, following the generated `AGENTS.md`, then use Git for review and
publication.

## Understand the source handoff

| Mode | Content source | Uncommitted edits | Writes from GitNodes |
|---|---|---:|---:|
| `preview` | local working tree | visible after refresh | no |
| `mcp` | local working tree | visible on the next tool call | no |
| local `serve` | GitHub repository and branch | not visible | yes, permission-aware |
| hosted deployment | GitHub repository and branch | not applicable | yes, permission-aware |

This distinction is intentional: local exploration stays offline, while shared
usage has a single remote source of truth and ordinary Git history.

## Publish and serve it

Stop preview with `Ctrl-C`, then commit and push:

```bash
git add .
git commit -m "Initialize GitNodes knowledge base"
gh auth login
gh repo create my-brain --private --source=. --remote=origin --push
gitnodes serve
```

`serve` discovers `owner/repo` from `remote.origin.url`, discovers the current
branch, and reuses `gh auth token` in process memory. It does not copy the token
into `.env`.

The local server is single-user PAT mode and binds to loopback by default.
Users with `push` can commit directly; other contributors use the pull-request
fallback where that mutation supports it.

Before switching modes, use this short check:

```bash
git status
git push
gitnodes serve
```

If `serve` shows older content, first verify that the local commit was pushed to
the branch printed at startup.

## Next steps

- Adapt node types and relationships in [Configuration](CONFIGURATION.md).
- Review the exact mode and write-path support in the
  [Feature inventory](../FEATURES.md).
- Move from loopback use to a persistent service with
  [Deployment](DEPLOYMENT.md).

