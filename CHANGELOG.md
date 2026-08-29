# Changelog

Notable user-visible changes are recorded here. GitNodes follows semantic
versioning once public releases begin.

## Unreleased

## 0.2.0 — unreleased

A consolidation release: no new subsystems, but the packaging, dependencies,
demo, and documentation are all brought back in line with what GitNodes
actually is.

### Added

- [Agent walkthrough](docs/guides/AGENT_WALKTHROUGH.md) — an end-to-end trace of
  an agent reconstructing a decision over MCP, with real tool calls and output.
- WinGet install instructions (`winget install AndreaBozzo.GitNodes`). The
  package has been live in `microsoft/winget-pkgs` since 0.1.0; only the docs
  said otherwise.

### Changed

- **The demo brain is now an engineering knowledge base.** `examples/demo-brain`
  replaces "The Meridian Program" (a fictional space program) with "Harbor
  Engineering": 37 notes of ADRs, systems, incidents, runbooks, postmortems, and
  issues, whose central thread is a decision that caused an outage that produced
  the decision reversing it. The old brain demonstrated typed edges; this one
  demonstrates why you would want them.
- README now leads with engineering knowledge and agent traversal rather than
  generic note-taking.
- `packaging/README.md` and `docs/ROADMAP.md` describe Homebrew and WinGet as
  published, which they are.
- `AGENTS.md` lists the `doctor` and `preview` subcommands it had been omitting.

### Dependencies

- Migrated to reqwest 0.13. `RequestBuilder::query` and `::form` are now
  opt-in features and are enabled per crate. The default TLS backend moves from
  native-tls to rustls, which **removes OpenSSL from the dependency tree
  entirely** — one less system library for the single-file binary to find.
- Updated ammonia to 4.1.4 for RUSTSEC-2026-0213 (XSS via SVG `animate`/`set`
  tags, which bypassed attribute URL sanitisation). Ammonia is the HTML/CSS
  sanitiser on the markdown rendering path.
- Updated h2 to 0.4.19 for RUSTSEC-2026-0258 (unbounded empty DATA frames), and
  event-listener to 5.4.2 for RUSTSEC-2026-0221 (unsound `Send`/`Sync`).
- The `cargo audit` CI job now requests `checks: write`. It had been failing on
  "Resource not accessible by integration" — a permissions error, not a finding —
  which made a green audit indistinguishable from a broken one.
- Cleared the Dependabot backlog: tower-http 0.7, rmcp 2.2, time 0.3.53,
  http-body-util 0.1.4, tailwindcss 4.3.3, and five pinned GitHub Actions.

## 0.1.0 — 2026-07-01

First public release.

### Added

- Local read-only preview and MCP access over the working tree.
- Single-user GitHub CLI/PAT serving mode.
- Starter brain scaffolding and generated `AGENTS.md`.
- Single-file release builds and Homebrew/WinGet metadata generation.
- GitNodes visual identity and prism-style graph nodes.
- GitNodes wordmark in the app header, plus social/SEO metadata (description,
  Open Graph, and Twitter card tags) so shared links render with a title and image.
- End-to-end guides and an implementation-backed feature inventory.

### Changed

- Renamed the workspace and crates from Brain UI to GitNodes, including the
  user-facing commit messages and in-app copy that mutations write to GitHub.
