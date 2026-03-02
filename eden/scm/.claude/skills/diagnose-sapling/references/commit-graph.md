# Commit Graph Diagnosis

**Commit graph problems — history, DAG, commits, sync, commit/amend/fold operations**

## Run these first

```bash
sl doctor                    # repairs segments, metalog, mutation store
sl cloud status              # commit cloud workspace state
```

## Then check blackbox for relevant events

```bash
# Find slow commands in the last 24 hours
sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'

# Check network/EdenAPI performance
sl blackbox --start 60 --pattern '{"network": "_"}'

# Check commit cloud sync activity
sl blackbox --start 60
# Then look for [legacy][visibility] and CommitCloudSync events
```

**For any slow command, get all events for its PID:**
```bash
sl blackbox --start 1440 | grep "<PID>"
```
Then analyze the output using [blackbox-guide.md](blackbox-guide.md).

## Common commit graph problems

- **Slow `sl log` / `sl smartlog`** — check visibility head count in blackbox (`[visibility] read N heads`). 200-300 is normal; 1000+ is a problem. Fix: `sl hide` old drafts, `sl cloud cleanup`. Use `sl debugvisibleheads` to see what the actual draft heads are and decide what to hide.
- **Missing commits** — `sl doctor` repairs segments and metalog
- **Rebase/amend failures** — `sl doctor` repairs mutation store
- **Cloud sync not working** — check blackbox for CommitCloudSync errors. Fix: `sl cloud sync --force` or `sl cloud leave && sl cloud join`
- **Pull/push slow or failing** — check network timing in blackbox metrics JSON (see EdenAPI section in [blackbox-guide.md](blackbox-guide.md)). May be server-side — check Mononoke oncall.
- **Corrupt segments / DAG** — `sl doctor` rebuilds segments
- **Corrupt metalog** — `sl doctor` rebuilds from segments
- **"Commit graph v2 with empty common" / 500 Internal Server Error on pull or goto** — the local commit graph has diverged from the server to the point where they share no common commits. Fix: `sl debugrebuildchangelog` — this reclones the changelog from the server and copies over local draft commits. Note: this is destructive — it removes invisible commits and truncates metalog history. Run `sl rage` first to capture state.
- **Recovering from bad operations** — `sl debugundohistory -l` shows recent undo-able operations (amend, commit, rebase). Use `sl undo` to reverse them.

## Commit and amend problems

`sl commit` and `sl amend` straddle both domains: they read the working copy (status) and write to the commit graph (mutation store, cloud sync). Diagnose both sides.

**Run these first:**
```bash
sl doctor                    # repairs treestate, metalog, mutation store

# Check for slow commit/amend in the last 24 hours
sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'
# Then grep for commit/amend PIDs and inspect their events
```

**Check mutation history (who changed what, and how):**
```bash
# Show mutation chain for recent drafts (amend, rebase, fold, split, metaedit)
sl debugmutation -r 'draft() & date(-4)' -t 'since 4d ago'

# Show mutation history for a specific commit
sl debugmutation -r <hash>

# Show successors (what a commit was rewritten into)
sl debugmutation -r <hash> --successors
```

- Output shows a tree of predecessors: each commit's origin (amend, rebase, fold, importstack, split, metaedit)
- Useful for:
  - **Tracing commit evolution** — when someone says "lines are missing from my commit," walk the mutation chain to find which operation changed the content. Then `sl diff -r <predecessor> -r <successor>` to see exactly what changed at each step.
  - **Finding the original version** of a commit before it was amended/rebased
  - **Diagnosing "lost" commits** that were actually rewritten into a new hash
  - **Understanding fold/split results** — seeing which commits were combined or split

**Check metalog for commit visibility changes:**
```bash
# Show how draft heads were added/removed over the last 2 days
sl debugmetalog -t 'since 2d ago'
```

**Check commit cloud state and history:**
```bash
# Current workspace state (sync status, head count, last sync time)
sl cloud status

# Historical smartlog — jump to a specific date (INTERACTIVE — recommend using for user, not from skill)
sl cloud sl --history --date 2025-08-20
```

**Check changelog backend:**
```bash
sl debugchangelog
```
- Shows which storage backends are in use (segments, zstore, SaplingRemoteAPI)
- Useful for verifying the changelog is in the expected state (lazy vs full)

## Common commit/amend problems

- **Slow `sl commit` / `sl amend`** — usually caused by slow `sl status` (runs internally). Check watchman events in blackbox. Also check for pre-commit hooks: `sl blackbox --start 60 --pattern '{"blocked": "_"}'` shows time waiting on hooks/editors.
- **`sl amend` slow with child commits** — amend triggers automatic restack (rebase of child commits). More children = more work. Check blackbox for rebase events after the amend.
- **`sl fold` failures** — `sl fold --from` folds linearly from current commit to specified commit. Common issues: commits are not linear (have branches between them), merge conflicts during fold. Check `sl log -r 'draft()'` to verify the commit chain is linear.
- **`sl fold` slow** — folding many commits requires reading all their changes and combining. If child commits exist below the fold target, automatic restack runs afterward (use `--no-rebase` to skip if not needed).
- **Commit appears lost** — check `sl debugmutation` to see if it was rewritten (amend/rebase/fold). Check `sl debugmetalog` to see if it was removed by cloud sync. Check `sl cloud sl --history --date <date>` to see the smartlog at the time it was last seen.
- **Missing lines or unexpected content in a commit** — use `sl debugmutation -r <hash>` to trace the commit's evolution. Walk the mutation chain and diff each step: `sl diff -r <predecessor> -r <successor>` to find which operation changed the content.
- **Cloud sync creating duplicate commits** — check `sl debugmetalog` for competing add/remove operations from different cloud sync versions. Fix: `sl cloud sync --force`.
- **Mutation history lost** — `sl doctor` repairs mutation store.
