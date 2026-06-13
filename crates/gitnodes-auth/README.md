# gitnodes-auth

GitHub authentication primitives for GitNodes: OAuth handshake helpers,
organization-membership checks, and session token storage.

Authorization itself (per-request `pull`/`push`/`admin` repository permissions)
lives in `gitnodes-app`; this crate provides the building blocks it composes.
Works org-less on personal repos; `GITHUB_LOGIN_ORG` optionally gates login.

Depends only on [`gitnodes-domain`](../gitnodes-domain). Apache-2.0. Part of the
[GitNodes workspace](../../README.md).
