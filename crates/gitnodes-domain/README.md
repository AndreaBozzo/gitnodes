# gitnodes-domain

Pure, WASM-safe domain types for GitNodes. No I/O, no async — the foundation
every other crate builds on.

Holds `BrainConfig` (the `.gitnodes.yml` contract), `Node`, `Edge`,
`WorkItem`, `TargetRef`/`TargetConfig`, and the editorial/domain frontmatter
split. Also the `GithubClient` URL builder and typed errors (`BrainError`,
`ConflictKind`).

Apache-2.0. Part of the [GitNodes workspace](../../README.md).
