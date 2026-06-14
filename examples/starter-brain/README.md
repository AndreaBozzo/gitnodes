# Starter brain

A minimal example of a GitNodes knowledge base: a `.gitnodes.yml` config and a
handful of linked markdown notes (two concepts, one ADR, one project).

## Try it locally

From the GitNodes source checkout:

```bash
gitnodes preview examples/starter-brain
```

Open the graph and you'll see the notes connected by body links, the ADR's
typed `decides_on:` edge, and shared tags.

For a writable copy with its own Git history:

```bash
gitnodes init my-brain
cd my-brain
gitnodes preview
```

When ready for collaborative GitHub-backed editing, stop preview, commit and
push the brain, authenticate with `gh auth login`, then run `gitnodes serve`.
Preview reads the local working tree; serve reads the pushed GitHub branch.
Follow the full [getting-started guide](../../docs/guides/GETTING_STARTED.md).

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
