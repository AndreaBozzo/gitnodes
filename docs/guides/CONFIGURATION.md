# Configuration

A brain is a Git repository containing markdown files. `.gitnodes.yml` defines
how those files become nodes, edges, saved views, and operational work items.
When the file is absent, GitNodes uses a built-in default taxonomy. The legacy
`.brain-config.yml` filename remains a fallback.

## Node types

```yaml
default_type: concept

node_types:
  - name: concept
    label: Concept
    directory: concepts
    accent: "#2dd4bf"
    title_key: topic
    date_create_field: date_created
    date_update_field: date_updated
    body_label: Summary
    creatable: true
    template_filename: ConceptNote.md
    frontmatter_seed:
      status: draft
    link_fields:
      related_project: project

  - name: project
    label: Project
    directory: projects
    accent: "#38bdf8"
    title_key: name
```

| Field | Meaning |
|---|---|
| `name` | Canonical value stored in note frontmatter as `type:`. |
| `label` | Human-readable UI label. |
| `directory` | Repository directory for this type; empty for virtual types. |
| `accent` | Required `#RRGGBB` graph/UI color. |
| `creatable` | Whether the type appears in the new-node menu. |
| `template_filename` | Optional markdown file under `templates/`. |
| `frontmatter_seed` | Values inserted when creating a note. |
| `title_key` | Frontmatter key containing the title. |
| `date_create_field` | Field populated with today's date on create. |
| `date_update_field` | Field refreshed on every UI save. |
| `body_label` | Editor label for the markdown body. |
| `link_fields` | Frontmatter field to target-node-type mappings. |
| `work_item_kind` | Makes the node type an operational work item. |

After changing the taxonomy in a local brain, regenerate its agent instructions:

```bash
gitnodes agents .
```

## Notes and links

Every note may start with YAML frontmatter:

```markdown
---
type: concept
topic: Knowledge graphs
tags: [knowledge, graph]
cover: assets/2026/06/knowledge-graph.png
cover_alt: A connected graph
related_project: graph-browser
---

# Knowledge graphs

See [Graph browser](../projects/graph-browser.md).
```

GitNodes builds edges from:

- standard relative markdown links in the body;
- fields configured under `link_fields`;
- shared tags.

Wikilinks such as `[[other note]]` are not parsed. Unresolved typed links are
ignored so future relationships can be drafted without breaking the graph.

Unknown frontmatter fields are preserved semantically on UI save, but YAML
comments, key order, and quoting style are not preserved. Malformed frontmatter
blocks UI saving until corrected.

## Saved views

```yaml
views:
  - name: Active decisions
    slug: active-decisions
    types: [adr]
    tags: [active]
    weight: -10
```

Views reuse the graph's URL-backed type and tag filters. Slugs must be unique
and contain lowercase letters, digits, `-`, or `_`. Tags must be lowercase.
Lower `weight` values appear first.

Administrators can preview the before/after YAML and save views through the
admin UI. The save is protected by the config file SHA and uses direct commit
or pull-request fallback.

## Work items

Set `work_item_kind` on a node type. Supported kinds are `task`, `discussion`,
`decision`, `incident`, `change`, and `quote`. Supported states are `backlog`,
`todo`, `in-progress`, `blocked`, `done`, and `cancelled`.

Typical work-item frontmatter:

```yaml
brain_id: task-api-read
status: in-progress
assignees: [andrea]
system_of_record: split
external_binding:
  system: github
  project: owner/repository
  item_key: "42"
  url: https://github.com/owner/repository/issues/42
```

`system_of_record` controls synchronization:

| Value | Behavior |
|---|---|
| `brain` | Markdown is authoritative; provider events do not overwrite it. |
| `external` | The bound provider is authoritative for synchronized fields. |
| `split` | GitNodes coordinates changes in both directions. |

The domain model can represent GitHub, GitLab, Gitea, Forgejo, and custom
bindings. Current provider synchronization and comment loading are implemented
for GitHub.

`label_taxonomy` maps work-item kinds and states to provider labels:

```yaml
label_taxonomy:
  - kind: task
    kind_label: "brain:task"
    state_labels:
      todo: "brain:todo"
      in-progress: "brain:in-progress"
      done: "brain:done"
```

## Runtime configuration

Local `gitnodes serve [dir]` normally needs no `.env`: it discovers the target
from Git and reuses `gh auth`. Explicit environment variables take precedence.

For hosted configuration, authentication modes, persistence, webhooks, and the
complete operator environment table, see [Deployment](DEPLOYMENT.md).

