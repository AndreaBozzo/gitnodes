# Starter brain

A minimal example of a GitNodes knowledge base: a `.gitnodes.yml` config and a
handful of linked markdown notes (two concepts, one ADR, one project).

## Use it as a template

The fastest way to get a repo GitNodes can read:

1. Create a new GitHub repository.
2. Copy the contents of this folder into it (including `.gitnodes.yml`).
3. Point GitNodes at `owner/your-new-repo` via `TARGET_GITHUB_REPOSITORY`.

Open the graph and you'll see the notes connected by body links, the ADR's
typed `decides_on:` edge, and shared tags.

## What to look at

- [.gitnodes.yml](.gitnodes.yml) — the config: node types, colours, a typed
  `link_fields` edge, and a saved view.
- [adrs/0001-git-as-source-of-truth.md](adrs/0001-git-as-source-of-truth.md) —
  shows a typed edge (`decides_on:`) plus body links.
- Any note — every file is plain markdown with a YAML frontmatter block. Delete
  `.gitnodes.yml` entirely and GitNodes still works, using its built-in default
  taxonomy.
- [AGENTS.md](AGENTS.md) — generated from `.gitnodes.yml` by `gitnodes agents`.
  It teaches coding agents (Claude Code, Codex, Cursor, …) this brain's
  conventions so they can add and link notes correctly. Regenerate it whenever
  you change the config.
