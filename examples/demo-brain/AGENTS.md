# AGENTS.md

This repository is a **GitNodes knowledge base**: a graph of markdown notes that humans and AI agents both read and edit. Git is the source of truth — every note is a plain markdown file with a YAML frontmatter block, and edits are ordinary commits or pull requests.

> Generated from `.gitnodes.yml` by `gitnodes agents`. Re-run it after changing the config.

## Node types

Each note declares a `type:` in its frontmatter; that decides which folder it belongs in and how it is styled.

- **mission** → `missions/` — title in `title:`; typed links: `lead:` → person, `operated_by:` → org, `program:` → program, `spacecraft:` → spacecraft, `target:` → body
- **spacecraft** → `spacecraft/` — title in `title:`; typed links: `built_by:` → org, `instruments:` → instrument, `mission:` → mission
- **body** → `bodies/` — title in `title:`; typed links: `orbits:` → body
- **instrument** → `instruments/` — title in `title:`; typed links: `aboard:` → spacecraft, `made_by:` → org
- **person** → `people/` — title in `title:`; typed links: `affiliation:` → org
- **org** → `orgs/` — title in `title:`
- **discovery** → `discoveries/` — title in `title:`; typed links: `body:` → body, `mission:` → mission
- **concept** → `concepts/` — title in `title:`
- **program** → `programs/` — title in `title:`

When unsure which type to use, default to `concept`.

## Frontmatter

Every note begins with a fenced YAML block:

```yaml
---
type: <one of the types above>
# put the human title under that type's title key (e.g. topic: or name:)
tags: [optional, tags]
---
```

- `type:` must match a node type above.
- Unknown keys are preserved untouched on save — safe to add custom fields.
- A malformed YAML block blocks saving, so keep it valid.

## Linking notes

- Use **standard markdown links**: `[Other note](../concepts/other-note.md)`.
- Do **not** use `[[wikilinks]]` — GitNodes does not parse them.
- Typed edges come from the `link_fields` listed above (a frontmatter field whose value is the path or slug of another note).
- Shared `tags:` cluster related notes in the graph.

## Adding a note

1. Pick the right `type` and create the file in that type's directory.
2. Write valid frontmatter (type + title + any seed fields), then the body in markdown.
3. Link it to related notes with standard markdown links.
4. Commit. The graph rebuilds from the repository.

## Agent tools

When the `gitnodes` MCP server is configured in your agent, prefer its read-only `search_brain`, `list_nodes`, `read_node`, `node_links`, and `validate_brain` tools for discovery and health checks. They read the current working tree through the same projection and search engine as the GitNodes UI. Use `node_links` to walk the graph from a note to its incoming and outgoing connections instead of guessing relationships from the text.

### Connecting the MCP server

The command is the same for every client — `gitnodes mcp <path-to-this-repo>`; only where the config lives differs. One-line setup for CLI agents:

```bash
# Claude Code
claude mcp add gitnodes -- gitnodes mcp "/home/andrea/Documenti/Brain_UI/examples/demo-brain"
# Codex CLI
codex mcp add gitnodes -- gitnodes mcp "/home/andrea/Documenti/Brain_UI/examples/demo-brain"
```

For editors that use a JSON config (Cursor, Antigravity, Cline, Windsurf, Claude Desktop, …), add the standard `mcpServers` entry to your client's config file:

```json
{
  "mcpServers": {
    "gitnodes": {
      "command": "gitnodes",
      "args": ["mcp", "/home/andrea/Documenti/Brain_UI/examples/demo-brain"]
    }
  }
}
```

See your client's MCP documentation for the exact config-file location.
