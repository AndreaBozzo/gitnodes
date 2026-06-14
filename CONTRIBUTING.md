# Contributing to GitNodes

GitNodes accepts focused bug fixes, documentation improvements, and
implementation work aligned with [the roadmap](docs/ROADMAP.md). Before a large
feature or architectural change, open an issue describing the user problem and
the proposed boundary.

## Development setup

Install Rust, the `wasm32-unknown-unknown` target, Node 20+, `cargo-leptos
0.3.6`, and optionally `just`.

```bash
just setup
just css-watch
just dev
```

Run the complete CI gate before submitting:

```bash
just check
```

Release-sensitive changes should also run:

```bash
just build
just embed-check
just deny
```

See [AGENTS.md](AGENTS.md) for architecture, invariants, and test commands.

## Change guidelines

- Keep Git as the only content source of truth. Never update the SQLite
  projection alongside a repository mutation.
- Preserve the strict dependency direction: app -> storage/auth -> graph ->
  domain.
- Gate server-only dependencies and code behind the `ssr` feature.
- Add focused tests for behavior changes. GitHub API tests use `wiremock`;
  projection tests use in-memory SQLite.
- Keep unrelated refactors and generated churn out of a change.
- Document user-visible capabilities and limitations in
  [docs/FEATURES.md](docs/FEATURES.md).

## Pull requests

Describe:

- the user problem;
- the implementation and important tradeoffs;
- verification performed;
- documentation, migration, security, or deployment impact.

Pull requests should be reviewable as one coherent change. Maintainers may ask
for large changes to be split into smaller slices.

By contributing, you agree that your contribution is licensed under the
license applicable to the files you modify: Apache-2.0 for the library crates
and AGPL-3.0-or-later for the deployable application.
