# The Meridian Program — a GitNodes demo brain

This is a small, self-contained knowledge base used to demo [GitNodes](https://github.com/AndreaBozzo/gitnodes).
It describes a fictional near-future deep-space exploration program: missions
that target worlds, fly spacecraft, carry instruments, are run by people and
organisations, and produce discoveries.

Nothing here is real — it exists to show what GitNodes does with an ordinary
folder of markdown. The relationships you see in the graph (a mission's target,
a spacecraft's instruments, a moon's parent planet) all come from plain YAML
frontmatter, declared as typed edges in [`.gitnodes.yml`](.gitnodes.yml).

## Try it locally

```bash
gitnodes preview .     # read-only graph in your browser, no GitHub, no login
gitnodes mcp .         # same notes, exposed to AI agents over stdio
```

## What to look at

- Open the **Tour: start here** saved view in the sidebar.
- Toggle the edge kinds in the bottom-left legend to isolate *body* links from
  typed *frontmatter* relationships (target, spacecraft, orbits, …).
- Search for `ocean` and follow the links out of Europa.
- Click [Europa](bodies/europa.md) and walk to its ocean
  [discovery](discoveries/europa-subsurface-ocean.md), the [mission](missions/tidewater.md)
  that found it, and the [radar](instruments/ice-radar.md) that measured it.
