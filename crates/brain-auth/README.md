# brain-auth

GitHub authentication primitives for Brain UI: OAuth handshake helpers,
organization-membership checks, and session token storage.

Authorization itself (per-request `pull`/`push`/`admin` repository permissions)
lives in `brain-app`; this crate provides the building blocks it composes.
Works org-less on personal repos; `GITHUB_LOGIN_ORG` optionally gates login.

Depends only on [`brain-domain`](../brain-domain). Apache-2.0. Part of the
[Brain UI workspace](../../README.md).
