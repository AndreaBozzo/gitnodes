# Presentation Demo Runbook

Target length: 5-7 minutes.

Demo target: Pokemon sandbox Brain, using the canonical multi-target route and
the current branch selected in Brain Switcher.

## Flow

1. Open the Pokemon Brain from Brain Switcher.
   - Confirm the header shows the active target, indexed file/node/type counts,
     and a calm live status.
   - Use Refresh only if the target was changed outside the app during prep.

2. Start from a saved view.
   - Pick the view that narrows the taxonomy to the demo path.
   - Show that Structure, Types, and Tags all update the same graph scope.

3. Navigate graph to detail.
   - Select a central Pokemon, route, quest, or strategy node.
   - Use the detail header to show target path, GitHub source, backlinks, and
     rendered Markdown without opening the editor yet.

4. Show a bound work item.
   - Open a node that has a Brain work item bound to a GitHub issue or PR.
   - Point out Brain ID, provider binding, source-of-record, status, assignees,
     and the sync note.
   - Open GitHub comments if the issue has QA discussion.

5. Explain operational status.
   - Open Status from the Knowledge header.
   - Show projection readiness, schema version, indexed counts, rebuild cost,
     pending provider sync, sessions, and audit log.

6. Contributor proposal flow.
   - Return to Knowledge.
   - Make a small edit or work-item mutation as the limited contributor.
   - If direct write is unavailable, show the PR fallback notice and explain
     that the live Brain updates after merge.

## Prep Checklist

- Saved views are named for the story being told, not internal implementation.
- At least one work item is bound to a GitHub issue or PR with a visible URL.
- The selected path has clean backlinks and no accidental orphan warnings.
- Projection status is ready before starting; pending provider sync is either
  empty or intentionally used as an operational example.
- The contributor branch/PR is reset to a known small change before rehearsal.
