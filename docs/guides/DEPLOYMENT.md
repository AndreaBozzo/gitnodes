# Deployment

Use `gitnodes serve` for private loopback use. Use a persistent hosted
deployment when multiple people need OAuth login, webhook-driven refresh, or a
stable shared URL.

## Local GitHub-backed server

From a pushed GitHub checkout:

```bash
gh auth login
gitnodes serve
```

The command discovers the GitHub repository from `origin`, the current branch
from Git, and a token from GitHub CLI. This activates single-user PAT mode and
refuses a non-loopback bind unless `GITNODES_ALLOW_REMOTE_PAT=1` is explicitly
set.

Do not expose PAT mode directly to untrusted users. If remote PAT mode is
intentional, place it behind your own TLS and access-control layer.

## Hosted authentication modes

| Mode | Required values | Intended use |
|---|---|---|
| OAuth | `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, target repository | Multi-user deployment. Callback URL is `{host}/auth/callback`. |
| PAT | `GITHUB_PAT`, target repository | Deliberate single-user deployment. Loopback-only unless explicitly overridden. |
| GitHub CLI | Git checkout plus `gh auth login` | Local `serve`; token remains process-local. |

Every request still checks live repository permissions. `pull` permits reads;
`push` enables direct writes and merge actions. `GITHUB_LOGIN_ORG` may restrict
who can sign in, but it does not replace repository authorization.

## Target selection

Prefer:

```dotenv
TARGET_GITHUB_REPOSITORY=owner/repository
TARGET_GITHUB_BRANCH=main
```

Older split variables (`TARGET_GITHUB_ORG`, `TARGET_GITHUB_REPO` and legacy
`GITHUB_*` aliases) remain compatible. Canonical multi-target URLs are
`/{owner}/{repo}/{branch}/knowledge`; confirmed brains are recorded in the
target registry and appear in the brain switcher.

## Docker

```bash
docker build -t gitnodes .
docker run --rm -p 3000:3000 \
  -e GITHUB_CLIENT_ID=... \
  -e GITHUB_CLIENT_SECRET=... \
  -e TARGET_GITHUB_REPOSITORY=owner/repository \
  -e SESSION_COOKIE_SECURE=0 \
  -v gitnodes_data:/app/data \
  gitnodes
```

`SESSION_COOKIE_SECURE=0` is only for local HTTP testing. Production should use
TLS and secure cookies.

Persist `data/`. It contains the SQLite session/audit/projection store and the
generated session encryption key. The content projection can be rebuilt from
Git; sessions and audit history cannot.

## Webhooks and background sync

Set `WEBHOOK_SECRET` to the same secret configured for GitHub's
`/webhook/github` endpoint. For background rebuilds and provider retries,
choose server-side credentials:

- GitHub App: `GITHUB_APP_ID`, `GITHUB_APP_INSTALLATION_ID`, and either
  `GITHUB_APP_PRIVATE_KEY` or `GITHUB_APP_PRIVATE_KEY_PATH`;
- fine-grained PAT fallback: `GITHUB_TOKEN` or `TARGET_GITHUB_TOKEN`.

Recommended GitHub App permissions are Contents read/write, Pull requests
read/write, Issues read/write, and Metadata read. App installation tokens are
preferred and refreshed automatically.

Without server-side credentials, authenticated user actions still work, but a
webhook can only mark the projection stale until a manual refresh.

## Operator environment

| Variable | Default | Purpose |
|---|---|---|
| `TARGET_GITHUB_REPOSITORY` | discovered locally | Default `owner/repository`. |
| `TARGET_GITHUB_BRANCH` | current branch locally, otherwise `main` | Target branch. |
| `GITHUB_PAT` | unset | Single-user request credential. |
| `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | unset | Multi-user OAuth credentials. |
| `GITHUB_LOGIN_ORG` | unset | Optional login organization restriction. |
| `LEPTOS_SITE_ADDR` | `127.0.0.1:3000` | Bind address; `PORT` is also accepted by hosted environments. |
| `LEPTOS_SITE_ROOT` | `target/site` | Static asset root for non-embedded builds. |
| `GITNODES_ALLOW_REMOTE_PAT` | unset | Allow PAT mode beyond loopback. |
| `GITNODES_ALLOW_REMOTE_PREVIEW` | unset | Allow read-only preview beyond loopback. |
| `GITNODES_NO_OPEN` | unset | Disable automatic browser opening on loopback. |
| `SESSION_DB_URL` | `sqlite://data/sessions.db` | SQLite session/projection database. |
| `SESSION_ENCRYPTION_KEY_FILE` | `data/session.key` | Persistent generated key path. |
| `SESSION_ENCRYPTION_KEY` | unset | Explicit base64 key, at least 64 decoded bytes. |
| `SESSION_COOKIE_SECURE` | enabled in release | Override secure-cookie behavior. |
| `WEBHOOK_SECRET` | unset | Enables verified GitHub webhooks. |
| `ALLOW_INSECURE_WEBHOOKS` | debug only | Development escape hatch for unsigned hooks. |
| `GITHUB_APP_*` | unset | GitHub App background credentials. |
| `GITHUB_TOKEN`, `TARGET_GITHUB_TOKEN` | unset | Background PAT fallback. |
| `GITHUB_API_BASE` | `https://api.github.com` | API base for App token minting. |
| `RATE_LIMIT_PER_SECOND` | `2` | Per-IP baseline rate. |
| `RATE_LIMIT_BURST` | `60` | Per-IP burst capacity. |
| `PENDING_SYNC_INTERVAL_SECS` | `60` | Provider outbox retry interval. |
| `RETENTION_INTERVAL_SECS` | `86400` | Session/audit retention sweep interval. |
| `AUDIT_RETENTION_DAYS` | `90` | Audit event retention; `0` removes older events each sweep. |
| `BRAND_NAME` | `GitNodes` | Header and page-title branding. |
| `BRAND_ORG_LABEL` | repository owner | Owner label in access messages. |
| `RUST_LOG` | `gitnodes_app=info,warn` | Tracing filter. |

## Health, routing, and proxying

- `GET /healthz` is an unauthenticated liveness probe.
- `GET /readyz` checks the stores and projection readiness and returns `503`
  with per-check details when unavailable.
- `/sse/events` carries target-scoped freshness events.
- `/assets/...` proxies private-repository images without exposing tokens.

Configure the reverse proxy to preserve the public host and scheme and to set a
trusted client IP header for rate limiting. WebSocket support is not required;
live updates use SSE.

## Releases and package managers

The release workflow builds self-contained binaries for Linux x86-64, macOS
x86-64/ARM64, and Windows x86-64, then publishes checksums and rendered
Homebrew/WinGet metadata.

The installer scripts, tap formula template, and WinGet templates are ready,
but public downloads and package-manager publication depend on the new public
upstream and its first release. Homebrew tap and WinGet submission remain
explicit publication steps; generating release metadata does not publish those
packages automatically.

For failure recovery, see [Operator notes](../OPERATOR_NOTES.md).

