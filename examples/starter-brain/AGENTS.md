# AGENTS.md

This repository is a **GitNodes knowledge base**: a graph of markdown notes that humans and AI agents both read and edit. Git is the source of truth — every note is a plain markdown file with a YAML frontmatter block, and edits are ordinary commits or pull requests.

> Generated from `.gitnodes.yml` by `gitnodes agents`. Re-run it after changing the config.

## Node types

Each note declares a `type:` in its frontmatter; that decides which folder it belongs in and how it is styled.

- **concept** → `concepts/` — title in `topic:`
- **adr** → `adrs/`; typed links: `decides_on:` → concept; seed fields: status
- **project** → `projects/` — title in `name:`; seed fields: status

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
