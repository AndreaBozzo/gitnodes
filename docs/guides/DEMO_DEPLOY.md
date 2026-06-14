# Hosting the public demo

GitNodes ships a read-only demo brain ([`examples/demo-brain`](../../examples/demo-brain),
"The Meridian Program") and the production `Dockerfile` can serve it with no
GitHub credential, no login, and an in-memory database. This is the safest
possible public surface: `preview` mode has no write path and holds no secrets.

## What makes this safe

- **No auth, no writes.** `gitnodes preview` serves the graph read-only; there is
  no editor, no commit path, and no admin surface (those routes redirect to the
  graph).
- **No credentials.** Preview never calls GitHub, so there is no token to leak.
- **Stateless.** The projection and sessions are in-memory; a restart is a clean
  slate. No volume required.

## Deploy on Railway

1. Create a new service from this repository. Railway builds the root
   `Dockerfile` automatically.
2. Set two environment variables on the service:

   | Variable | Value | Why |
   | --- | --- | --- |
   | `GITNODES_PREVIEW_DIR` | `/app/demo-brain` | Switches the image into read-only preview mode and points it at the bundled demo brain. |
   | `GITNODES_ALLOW_REMOTE_PREVIEW` | `1` | Acknowledges that preview binds a public (non-loopback) address. Without it the process refuses to start, by design. |

   `PORT` is provided by Railway and picked up automatically. Do **not** set
   GitHub or session variables — the demo needs none.
3. Deploy, then attach a domain (e.g. `demo.gitnodes.<your-domain>`).

That's it. A 256–512 MB instance is plenty; scale-to-zero is fine (first visitor
absorbs a ~2–3 s cold start). Put Cloudflare in front if you want edge caching
and basic abuse protection — the built-in per-IP rate limiter
(`RATE_LIMIT_PER_SECOND`, default 2) is the floor.

## Local smoke test

Reproduce exactly what the demo serves:

```bash
docker build -t gitnodes .
docker run --rm -p 3000:3000 \
  -e GITNODES_PREVIEW_DIR=/app/demo-brain \
  -e GITNODES_ALLOW_REMOTE_PREVIEW=1 \
  gitnodes
# open http://localhost:3000/knowledge
```

## Updating the demo content

The demo is just markdown in `examples/demo-brain`. Edit the notes (or run
`gitnodes doctor examples/demo-brain` after changes), commit, and redeploy — the
brain is copied into the image at build time.
