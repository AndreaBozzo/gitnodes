# Walkthrough: an agent reconstructing a decision

This guide follows one question through the
[demo brain](../../examples/demo-brain) over MCP:

> **Why does `api-gateway` depend on Redis, and what did that decision cost us?**

It is an ordinary question. Answering it means connecting a decision record from
2025, an outage from 2026, the postmortem in between, and the tickets that came
out of it — four documents that, in most organisations, live in four different
systems and are joined only inside somebody's head.

Everything below is a real tool call against `examples/demo-brain` and its real
result. Run them yourself:

```bash
gitnodes mcp examples/demo-brain
```

## Why grep does not answer this

`grep -ril redis examples/demo-brain` returns fourteen files. It cannot tell you
which of them is the *decision*, which is the *consequence*, or that one of the
matches supersedes another. The relationships are the answer, and the
relationships are not in the text — they are in the frontmatter, as typed edges.

That is the difference the four MCP tools make: `search_brain` finds a place to
start, and `node_links` walks structure from there.

## 1. Find a place to start

```json
{"name": "search_brain", "arguments": {"query": "session eviction", "limit": 4}}
```

```
issues/144-alert-on-session-store-evictions.md   0.0328
postmortems/2026-03-11-checkout-session-loss.md  0.0161
runbooks/session-store-failover.md               0.0159
issues/142-enable-session-store-persistence.md   0.0156
```

Ranked full-text search over the same FTS5 projection the web UI uses. Useful,
and still just search: four documents about evictions, with no indication of how
they relate. An agent that stops here writes a summary. An agent that continues
finds the cause.

## 2. Walk the structure

The postmortem is the highest-value hit, so ask what touches it:

```json
{"name": "node_links", "arguments": {"path": "postmortems/2026-03-11-checkout-session-loss.md"}}
```

The full result also carries every prose link; the typed edges are the ones that
carry meaning, and they come back with their field names intact:

```
outgoing  frontmatter:incident               incidents/2026-03-11-checkout-session-loss.md
outgoing  frontmatter:root_cause_in          systems/session-store.md
outgoing  frontmatter:contributing_decision  decisions/0004-session-state-in-redis.md
outgoing  frontmatter:action_items           issues/142-…  143-…  144-…  145-…
outgoing  frontmatter:written_by             people/dana-okonkwo.md
```

`contributing_decision` is the edge that matters. It is not a phrase in the prose
that a model has to infer — it is a declared relationship, so the agent knows
*that* ADR-0004 is implicated without having to decide whether it is.

## 3. Follow it back to the decision

```json
{"name": "node_links", "arguments": {"path": "decisions/0004-session-state-in-redis.md"}}
```

```
outgoing  frontmatter:affects      systems/api-gateway.md
outgoing  frontmatter:affects      systems/session-store.md
outgoing  frontmatter:owned_by     teams/platform.md
incoming  frontmatter:supersedes   decisions/0009-session-state-in-signed-cookies.md
incoming  body                     postmortems/2026-03-11-checkout-session-loss.md
incoming  body                     issues/145-revisit-session-state-strategy.md
```

Again trimmed to the typed edges. Two things arrive here that no amount of
reading ADR-0004 alone would give you.

The **incoming** `supersedes` edge says this decision has already been reversed —
information that lives entirely in a *different* document. A reader who opened
ADR-0004 from a search result would have no way to know it is dead.

The `affects` edges say precisely which systems inherited the constraint, so
"what did it cost us" has a bounded answer instead of a guess.

## 4. Collect the cost

```json
{"name": "list_nodes", "arguments": {"node_types": ["issue"], "limit": 10}}
```

```
#142  Enable session-store persistence and isolate batch workloads   [outage-2026-03-11]
#143  Rewrite the session-store failover runbook                     [outage-2026-03-11]
#144  Alert on session-store evictions, not checkout errors          [outage-2026-03-11]
#145  Revisit the session-state strategy                    [featured, outage-2026-03-11]
#151  Alert on PostgreSQL replication lag                                            [ ]
#158  Give notification lateness a number                                            [ ]
```

Four of the six issues in the repository trace back to this one decision. That
ratio *is* the cost, and it is a structural fact the graph can state, not an
impression assembled from prose.

## The answer

An agent that made those four calls can now say something a summary cannot:

> `api-gateway` depends on Redis because ADR-0004 (January 2025) moved session
> state out of process memory to make the gateway stateless and deploys
> non-disruptive. It succeeded at that — deploy frequency went from weekly to
> ~40×/week — but it declared a new tier-1 dependency without declaring what that
> dependency had to guarantee. Fourteen months later a batch job filled the same
> Redis cluster and its LRU policy evicted live sessions, logging out ~40% of
> users for 96 minutes.
>
> It cost four engineering tickets and, ultimately, the decision itself:
> ADR-0009 supersedes it, moving sessions to signed cookies. Note that ADR-0004
> is no longer in force — if you are reading it as current guidance, stop.

That last line is the one worth paying attention to. It is only available because
`supersedes` is a typed edge pointing backwards, and it is exactly the kind of
thing an agent grepping a documentation folder gets confidently wrong.

## Wiring it into your own agent

The demo brain ships agent config for Claude Code, Cursor, Codex, and
Antigravity, so opening the folder is enough:

```bash
cd examples/demo-brain
claude          # .mcp.json is picked up automatically
```

For any other MCP client, the server is a plain stdio command:

```json
{
  "mcpServers": {
    "gitnodes": { "command": "gitnodes", "args": ["mcp", "/path/to/your/brain"] }
  }
}
```

It is read-only and never calls GitHub. See
[GETTING_STARTED.md](GETTING_STARTED.md) for the difference between `mcp`/`preview`
(local working tree, including uncommitted files) and `serve` (the pushed GitHub
branch).

## Doing this to your own repository

The taxonomy that makes the walkthrough work is about sixty lines of
[`.gitnodes.yml`](../../examples/demo-brain/.gitnodes.yml) — node types, their
directories, and the `link_fields` that become typed edges:

```yaml
- name: postmortem
  directory: postmortems
  link_fields:
    incident: incident
    root_cause_in: system
    contributing_decision: adr   # the decision that set the trap
    action_items: issue          # what it produced (a list)
```

Nothing else is required. The notes stay ordinary markdown with YAML
frontmatter, readable and editable without GitNodes, and `gitnodes doctor`
tells you which links do not resolve. Start with the two or three relationships
you already reconstruct by hand most often.
