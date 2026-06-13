# gitnodes-graph

Builds the knowledge graph from a set of markdown files. Pure logic, WASM-safe
— no network, no storage.

```mermaid
flowchart LR
    md["Markdown + frontmatter"] --> edges["Typed edges<br/>Body / Frontmatter / Tag"]
    edges --> resolve["Link resolution"]
    resolve --> layout["Force-directed layout"]
    layout --> graph["Nodes + Edges"]
```

Edges are typed by origin: `Body` (inline links), `Frontmatter` (related/see-also
keys), and `Tag` (shared tags). Link resolution maps references to node ids; the
force-directed pass produces coordinates for the canvas.

Depends only on [`gitnodes-domain`](../gitnodes-domain). Apache-2.0. Part of the
[GitNodes workspace](../../README.md).
