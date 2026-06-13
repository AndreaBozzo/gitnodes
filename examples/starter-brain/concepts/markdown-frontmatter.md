---
type: concept
topic: Markdown frontmatter
date_created: 2026-01-01
tags: [basics, yaml]
---

# Markdown frontmatter

Frontmatter is the YAML block at the top of a note, fenced by `---`. GitNodes
reads it to decide a note's type, title, tags, and typed links — while the body
below stays plain markdown.

```yaml
---
type: concept
topic: My note
tags: [example]
---
```

The `type:` value matches a `node_type` in [.gitnodes.yml](../.gitnodes.yml),
which controls the note's folder, colour, and editing form. Keys GitNodes
doesn't recognise are preserved untouched on save.

## Related

- [Knowledge graph](knowledge-graph.md)
