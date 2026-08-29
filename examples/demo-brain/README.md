# Harbor Engineering — a GitNodes demo brain

A small, self-contained engineering knowledge base used to demo
[GitNodes](https://github.com/AndreaBozzo/gitnodes). Harbor is a fictional
payments company; nothing here is real.

What is real is the shape. This is the knowledge a working engineering team
actually accumulates — decisions, services, incidents, runbooks, postmortems,
tickets — and the thing that makes it valuable is not any single note. It is that
the notes point at each other:

```
ADR-0004  ──affects──▶  session-store  ──runbooks──▶  session-store-failover
   │                          │                              │
   │                          ▼                              │
   │                    11 March incident  ◀──runbook_used────┘
   │                          │
   │                          ▼
   └──contributing_decision── postmortem ──action_items──▶ #142 #143 #144 #145
                                                                          │
                                          ADR-0009  ◀──resulted_in────────┘
                                              │
                                              └──supersedes──▶ ADR-0004
```

That loop closes. A decision made in January 2025 to move session state into
Redis produced an outage in March 2026, whose postmortem produced four tickets,
one of which produced the decision that reversed the original. Reconstructing
that chain by hand means reading a wiki, a ticket tracker, and an incident log
and holding them in your head at once. Here it is four `node_links` calls.

## Try it locally

```bash
gitnodes preview .     # read-only graph in your browser, no GitHub, no login
gitnodes mcp .         # same notes, exposed to AI agents over stdio
```

This brain ships ready-to-use agent config (`CLAUDE.md`, `.mcp.json`, `.claude/`,
`.cursor/`, `.codex/config.toml`, `.agents/mcp_config.json`), so opening the
folder in Claude Code, Cursor, Codex, or Antigravity wires up the read-only
`gitnodes mcp` server automatically.

## What to look at

- Open the **Tour: start here** saved view in the sidebar, then **The 11 March
  outage** — the second one is the whole story above, filtered to the eleven
  notes that tell it.
- Toggle the edge kinds in the bottom-left legend to isolate prose *body* links
  from typed *frontmatter* relationships (`affects`, `supersedes`,
  `contributing_decision`, `action_items`, …). Only the typed ones are queryable.
- Start at [ADR-0004](decisions/0004-session-state-in-redis.md) and follow
  `affects` forward to the systems it constrained, then follow the incoming links
  back from [the postmortem](postmortems/2026-03-11-checkout-session-loss.md)
  that indicted it.
- Search for `eviction` and notice which notes come back: the incident, the
  runbook, the alerting ticket, and the decision that made evictions matter.

## Ask an agent

With the MCP server wired up, these are the questions the graph answers that a
folder of markdown does not:

> Why does api-gateway depend on Redis, and what did that decision cost us?

> Which runbooks belong to systems affected by a superseded ADR?

> Show me every issue that came out of a postmortem and is still open.

The [agent walkthrough](../../docs/guides/AGENT_WALKTHROUGH.md) traces the first
of those end to end, with the actual tool calls and the results they return.

## Deliberate rough edges

Some things here are wrong on purpose, because real knowledge bases are:

- [The session-store failover runbook](runbooks/session-store-failover.md) was
  stale during the incident that needed it. That is the finding, not an oversight.
- [The 2 May incident](incidents/2026-05-02-notification-backlog.md) has no
  postmortem, because a sev-3 does not get one. Its follow-up ticket had to be
  filed directly, and the graph shows that gap as a missing edge.
- [ADR-0002](decisions/0002-typed-events-between-services.md) records a
  consequence it never quantified, which is how
  [#158](issues/158-bound-notification-lateness.md) is still being filed twenty
  months later.
