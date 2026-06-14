# Security policy

## Supported versions

GitNodes is pre-release. Security fixes are applied to the latest code and the
most recent published release once releases begin.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability.

Use GitHub private vulnerability reporting:

<https://github.com/AndreaBozzo/gitnodes/security/advisories/new>

Include the affected version or commit, reproduction steps, impact, and any
suggested mitigation. Reports will be acknowledged as soon as practical.

## Security model

GitNodes handles GitHub credentials, private repository content, and
repository mutations. Important deployment expectations:

- use TLS and secure session cookies in production;
- persist and protect the session encryption key;
- verify webhooks with `WEBHOOK_SECRET`;
- keep PAT mode loopback-only unless it is behind independent authentication;
- grant GitHub Apps and tokens only the repository permissions they need;
- treat the target repository as authoritative and SQLite as disposable.

Operational controls and recovery guidance are documented in
[docs/OPERATOR_NOTES.md](docs/OPERATOR_NOTES.md).
