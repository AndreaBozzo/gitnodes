# GitNodes documentation

GitNodes serves two closely related workflows:

- humans browse and edit a Git-backed knowledge graph in the web UI;
- agents inspect the same markdown working tree through read-only MCP tools and
  make ordinary, reviewable Git edits.

## Choose a path

| Goal | Start here |
|---|---|
| Try GitNodes without GitHub or credentials | [Getting started](guides/GETTING_STARTED.md) |
| Move a local brain into collaborative GitHub-backed use | [Getting started: publish and serve](guides/GETTING_STARTED.md#publish-and-serve-it) |
| Define node types, links, views, and work items | [Configuration](guides/CONFIGURATION.md) |
| Run a persistent or multi-user instance | [Deployment](guides/DEPLOYMENT.md) |
| Check whether a capability exists or has limits | [Feature inventory](FEATURES.md) |
| Operate and recover a running instance | [Operator notes](OPERATOR_NOTES.md) |
| Understand the product direction | [Roadmap](ROADMAP.md) |

## The important boundary

`gitnodes preview` and `gitnodes mcp` read the local working tree, including
uncommitted changes. `gitnodes serve` and hosted deployments read the configured
GitHub repository and branch.

Commit and push before switching from preview to `serve`, otherwise the
GitHub-backed UI will correctly show the last pushed state rather than local
edits.

