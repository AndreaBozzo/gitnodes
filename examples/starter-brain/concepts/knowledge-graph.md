---
type: concept
topic: Knowledge graph
date_created: 2026-01-01
tags: [graph, basics]
---

# Knowledge graph

A knowledge graph is a set of notes (nodes) connected by the links between them
(edges). Instead of a flat folder of files, you navigate by relationship: a
decision points at the concepts it depends on, a project points at the people
and ideas involved.

GitNodes builds this graph from your markdown. Links come from two places:

- **Body links** — ordinary markdown links like [Markdown frontmatter](markdown-frontmatter.md).
- **Typed edges** — declared in `.gitnodes.yml` via `link_fields`, drawn from
  named frontmatter fields so you can tell an "owns" edge from a "cites" edge.

## Related

- [Markdown frontmatter](markdown-frontmatter.md)
