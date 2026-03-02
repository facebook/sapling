<objective>
Known Sapling and EdenFS problems, their symptoms, diagnostic evidence, and fixes. Use this as a lookup when diagnostic commands reveal a recognizable pattern.
</objective>

<working_copy_issues>
**Corrupt treestate / dirstate**

- Symptoms: `sl status` errors, incorrect file states, "treestate" errors in output
- Diagnosis: `sl doctor` reports treestate issues
- Fix: `sl doctor` auto-repairs treestate

**Corrupt metalog**

- Symptoms: commands fail with metalog-related errors, bookmarks/remotenames missing
- Diagnosis: `sl doctor` reports metalog corruption
- Fix: `sl doctor` rebuilds metalog from segments

**Corrupt mutation store**

- Symptoms: amend/rebase history lost, obsolete commits reappearing
- Diagnosis: `sl doctor` reports mutation store issues
- Fix: `sl doctor` repairs mutation store

**Stale watchman state**

- Symptoms: `sl status` is slow or returns stale results, watchman clock errors in blackbox
- Diagnosis: `sl blackbox --start 60 --pattern '{"watchman": "_"}'` shows errors or high duration
- Fix: `watchman watch-del-all && watchman shutdown-server`
</working_copy_issues>

<commit_graph_issues>
**Corrupt segments / DAG**

- Symptoms: commands fail with segment-related errors, missing commits
- Diagnosis: `sl doctor` reports segment issues
- Fix: `sl doctor` rebuilds segments

**Too many visible heads (excessive drafts)**

- Symptoms: commands that operate on all drafts are slow, high memory usage
- Diagnosis: `sl blackbox` shows `[visibility] read N heads` with very large N (1000+)
- Fix: `sl hide` old draft commits, or `sl cloud cleanup` to remove stale cloud synced commits
- Note: 200-300 heads is normal; this is only a problem at 1000+

**Commit cloud sync issues**

- Symptoms: drafts not syncing across devices, cloud sync errors
- Diagnosis: `sl blackbox --start 60` shows CommitCloudSync events with errors
- Fix: `sl cloud sync --force` or `sl cloud leave && sl cloud join`

**"Commit graph v2 with empty common" / 500 Internal Server Error**

- Symptoms: `sl pull`, `sl goto Dxxx`, or other network operations fail with 500 Internal Server Error from Mononoke: `"Commit graph v2 with empty common is not allowed"`
- Diagnosis: the local commit graph has diverged so far from the server that they share no common commits. The `commit/graph_v2` endpoint requires at least one common ancestor.
- Fix: `sl debugrebuildchangelog` — reclones the changelog from the server and copies local draft commits. **Warning**: this is destructive — removes invisible commits (including shelved changes based on them) and truncates metalog history. Run `sl rage` first to capture state.
- Note: you do not need to `sl pull` before `sl goto Dxxx` — Sapling has automatic pulling at the revset layer
</commit_graph_issues>

<commit_issues>
**Slow `sl commit` / `sl amend`**

- Symptoms: commit or amend takes many seconds
- Diagnosis: check blackbox for the PID — look at status timing (commit runs status internally), hook wait time (`{"blocked": "_"}`), and network timing
- Possible causes: slow `sl status` (stale watchman), slow pre-commit hooks, large changeset requiring uploads
- Fix: restart watchman if status is slow; check hooks; for large changesets, wait or optimize changeset size

**`sl fold` failures**

- Symptoms: fold fails with merge conflicts, "non-linear" errors, or unexpected results
- Diagnosis: `sl log -r 'draft()'` to verify commit chain is linear between fold endpoints; check for branches between commits
- Possible causes: non-linear commit chain (branches between fold source and target), merge conflicts when combining changes
- Fix: use `sl fold --exact` to fold only specific commits; resolve conflicts manually; ensure commits form a linear chain before folding

**`sl fold` slow**

- Symptoms: fold takes a long time, especially with many commits
- Diagnosis: check blackbox for fold PID events; look for rebase events after the fold (automatic restack of children)
- Possible causes: many commits being folded, large diffs, automatic restack of child commits
- Fix: use `--no-rebase` to skip restacking children if not needed

**`sl amend` with automatic restack**

- Symptoms: amend is slow because it also rebases child commits
- Diagnosis: blackbox shows rebase events immediately after the amend event for the same PID
- Possible causes: many child commits that need rebasing after the amend
- Fix: expected behavior; if not needed, consider using `sl amend --no-rebase`

**Lost or missing commits**

- Symptoms: a commit that existed before is no longer visible in smartlog
- Diagnosis: `sl debugmutation -r <hash>` to find if it was rewritten; `sl debugmetalog -t 'since 2d ago'` to see when it was removed; `sl cloud sl --history --date <date>` to browse historical smartlog
- Possible causes: amend/rebase/fold rewrote the commit (creating a new hash), cloud sync removed it, `sl hide` was run
- Fix: find the successor via `sl debugmutation`; if truly lost, check cloud history or `sl journal` for recent working copy changes

**Cloud sync duplicating or conflicting commits**

- Symptoms: duplicate commits appear, commits keep being added and removed
- Diagnosis: `sl debugmetalog -t 'since 2d ago'` shows rapid add/remove cycles from competing cloud sync versions
- Possible causes: multiple devices or terminals running conflicting operations, stale cloud workspace
- Fix: `sl cloud sync --force` or `sl cloud leave && sl cloud join` to reset workspace
</commit_issues>

<merge_driver_issues>
**"nested repo" error during rebase**

- Symptoms: `path 'www/...' is inside nested repo 'www'` during rebase
- Diagnosis: `ls -la ~/fbsource/www/.hg` — stale `.hg` directory exists
- Fix: `rm -rf ~/fbsource/www/.hg`

**Merge driver hangs during rebase**

- Symptoms: rebase appears stuck at "Running auto gen: <builder name>"
- Diagnosis: `sl debugprocesstree` to see what subprocess is running; Scuba `mergedriver_perf` for per-builder timing; warning printed if builder takes >5 seconds
- Fix: wait for completion; if too slow, kill and retry with `sl rebase -d <dest> --config experimental.mergedriver=` to disable merge drivers

**"Autogenerate failed" during rebase**

- Symptoms: rebase fails with `MergedriverFailedException` or `Autogen <name> failed`
- Diagnosis: run `sl rebase -d <dest> --config ui.debug=true` to see detailed builder output; check Scuba `autogen_failed` events
- Fix: investigate the builder's rebuild script; bypass with `--config experimental.mergedriver=`

**Silent change loss after rebase (CIGAR files)**

- Symptoms: rebased commit has unexpected content, missing changes in generated files
- Diagnosis: `sl debugmutation -r <hash>` to trace through rebase; `sl diff -r <predecessor> -r <successor>` to see exactly what changed
- Fix: identify which builder handled the file incorrectly; rebase manually with merge drivers disabled

**Merge conflicts not resolving (old mergebase)**

- Symptoms: www merge driver skips, conflicts remain unresolved
- Diagnosis: mergebase is >5 days old — the www merge driver auto-disables
- Fix: rebase from a more recent master first

**Merge driver errors hidden behind generic "unresolved conflicts" message**

- Symptoms: rebase fails with `InterventionRequired: unresolved conflicts`, but no actual merge driver error is visible in Scuba (`dev_command_timers` trace column)
- Diagnosis: the Scuba trace only shows the generic conflict error, not the underlying builder failure. This is a known gap — the merge driver error is swallowed before it reaches the Scuba logging.
- Fix: re-run the rebase with `--config ui.debug=true` to see the actual builder error output in the terminal. Check `FILES_TO_REBUILD` env var to understand which files triggered the rebuild. For www merge drivers, check www-specific Scuba tables for detailed output.
- Note: `ui.pushbuffer(error=True, tee=True)` in preprocess and `ui.popbuffer()` in conclude can be used to capture and log the real error — see `eden/scm/sapling/fb/mergedriver/__init__.py`

</merge_driver_issues>

<edenfs_issues>
**EdenFS not running**

- Symptoms: repo directory shows errors or is empty, `sl` commands fail with mount errors
- Diagnosis: `eden status` shows "not healthy" or "not running"
- Fix: `eden start` or `eden restart`

**Stale EdenFS mount**

- Symptoms: I/O errors in repo, files appear missing, operations hang, `Stale NFS file handle` (macOS) or `Transport endpoint is not connected` (Linux)
- Diagnosis: `eden doctor` reports mount issues; `mount | grep <repo>` shows stale kernel mount
- Fix escalation ladder:
  1. `eden doctor` auto-fixes
  2. `eden unmount <path> && eden mount <path>`
  3. Force-unmount stale kernel mount: `sudo umount -f <path>` (macOS) / `sudo umount -l <path>` (Linux), then `eden mount <path>`
  4. `eden rm <path>` + reclone (check OD backup first)
- See [edenfs-diagnosis.md](edenfs-diagnosis.md) for details

**Certificate expiration**

- Symptoms: network operations fail, EdenAPI errors, "certificate" or "x509" in error messages
- Diagnosis: `eden doctor` checks certificate at `/var/facebook/credentials/$USER/x509/$USER.pem`
- Fix: `update-certificates` or follow the cert renewal procedure

**High inode count / memory pressure**

- Symptoms: EdenFS using excessive memory, OOM kills
- Diagnosis: `eden stats` shows high loaded inode count, `eden top` shows memory usage
- Fix: `eden gc` to unload unused inodes, or restart EdenFS

**Corrupt backing store**

- Symptoms: checkout failures, file content errors, hash mismatches
- Diagnosis: `eden doctor` reports backing store issues
- Fix: `eden doctor` auto-repairs, may need to re-clone if severe

**EdenFS crash / unexpected restart**

- Symptoms: EdenFS stops responding, repo becomes inaccessible, `eden uptime` shows very short uptime
- Diagnosis: `eden debug log` to find crash evidence (fatal errors, stack traces, assertion failures, OOM). `eden uptime` shows how recently EdenFS restarted.
- Fix: `eden restart`. If crashes are recurring, check the log pattern and escalate with `eden rage`.

**EdenFS restart stuck**

- Symptoms: `eden restart` hangs and doesn't complete
- Diagnosis: `eden debug log` to see what's blocking the restart. May be stuck unmounting, waiting on in-progress operations, or blocked on thrift cleanup.
- Fix: `eden stop` first (force stop), then `eden start`. If stop also hangs, check for processes using the mount (`eden trace thrift --retroactive` to see active thrift calls).

**Redirection issues**

- Symptoms: `buck-out` or other redirected directories missing or broken, build failures due to missing bind mounts
- Diagnosis: `eden redirect list` shows redirection state. Broken redirections show as "not mounted" or have wrong target.
- Fix: `eden redirect fixup` to restore expected configuration. For specific broken redirections: `eden redirect del <redirection>` then re-add.

**Overlay corruption**

- Symptoms: file content errors, inconsistent directory state, unexpected behavior when reading/writing files
- Diagnosis: `eden fsck` scans the overlay for corruption. `eden fsck --check-only` for read-only scan, `eden fsck --verbose` for detailed output.
- Fix: `eden fsck` attempts repair. If severe, may need `eden doctor` or re-clone.

**FilteredFS / eden-sparse filter errors**

- Symptoms: `Failed to get filter` / `Failed to find a stored Filter for ID: V1(...)`, `UnexpectedMountProblem` from `eden doctor`, clone failures with filter errors
- Diagnosis: `edenfsctl fsconfig --all` to check filter config; `sl sparse show` for active profile; `sl status` to check if mount is functional
- Fix: `sl filteredfs switch <profile>` to re-apply filter (use `sl filteredfs`, NOT `sl sparse` — sparse commands abort on edensparse repos); if non-functional or backing repo mismatch, `eden rm` + reclone
- See [edenfs-diagnosis.md](edenfs-diagnosis.md) "FilteredFS / eden-sparse issues" for details
- Note: can occur on any platform, not just Windows
</edenfs_issues>

<network_issues>
**Slow Mononoke / EdenAPI**

- Symptoms: commands with many file/tree fetches are slow
- Diagnosis: blackbox metrics JSON shows high `request.time.ms` in the `edenapi` section, or `[network]` events with high `duration_ms`
- Fix: Usually a server-side issue; check Mononoke oncall. Can try `sl config --local paths.default` to verify the server URL.

**VPN / connectivity issues**

- Symptoms: all network operations fail or timeout
- Diagnosis: blackbox shows network errors or very high latency; `eden doctor` may report network issues
- Fix: Check VPN connection, try `curl -s https://mononoke.internal.tfbnw.net/health_check`
</network_issues>

<performance_issues>
**Slow `sl status`**

- Symptoms: `sl status` takes multiple seconds
- Diagnosis: check watchman events in blackbox (`--pattern '{"watchman": "_"}'`), check fsmonitor events
- Possible causes: stale watchman state, large number of modified files, EdenFS inode loading
- Fix: restart watchman, or check if EdenFS is healthy

**Slow `sl log` / `sl smartlog`**

- Symptoms: log/smartlog commands take many seconds
- Diagnosis: check blackbox for network timing (smartlog may fetch from server) and visibility head count
- Possible causes: many visible draft heads, network latency, server-side slowness
- Fix: hide old drafts, check network

**Slow checkout / update**

- Symptoms: `sl checkout` or `sl update` takes a long time
- Diagnosis: check blackbox for fetch stats in scmstore section — look for large numbers of cache misses
- Possible causes: large tree diff between commits requiring many file fetches, EdenFS invalidation overhead
- Fix: usually expected for large jumps; if EdenFS, check `eden doctor`

**Commands launched by automated processes**

- Symptoms: slow commands that the user did not run
- Diagnosis: `[process_tree]` in blackbox shows launchers like `devenv-backup`, `cron`, ISL
- Fix: depends on the launcher; may need to configure backup schedules, ISL polling intervals, etc.

</performance_issues>

<hang_issues>
**Command waiting on editor**

- Symptoms: command appears hung but is actually waiting for user input
- Diagnosis: `sl blackbox --start 60 --pattern '{"blocked": "_"}'` shows "waiting for editor"
- Fix: close the editor or terminal running the editor

**Command waiting on pre-commit hook**

- Symptoms: commit/amend appears stuck
- Diagnosis: `sl blackbox` blocked events show hook name and duration; `sl debugprocesstree` shows hook process
- Fix: wait for hook to complete; if hook is broken, investigate the hook script

**Lock contention between concurrent sl commands**

- Symptoms: command hangs waiting for lock, error about lock being held
- Diagnosis: `sl debugrunlog` shows another sl command currently running; `sl debugprocesstree` shows both processes
- Fix: wait for the other command to finish; if it's stuck, kill the holding process

**Network hang during fetch or push**

- Symptoms: command stuck with no progress
- Diagnosis: `sl debugrunlog` shows download_bytes not changing over time; `sl debugnetwork` may timeout
- Fix: check network connectivity; may need to kill and retry; check Mononoke oncall if server-side
</hang_issues>

<escalation_template>
**When the problem cannot be resolved, help the user write a support post with this structure:**

```
Subject: [sl <command>] <brief description of problem>

Environment:
- Devserver: <hostname>
- sl version: <output of sl --version>
- EdenFS version: <output of eden version>
- OS: <uname -r>

Problem:
<1-2 sentence description>

Diagnostic data:
<Paste the blackbox output for the relevant PID>

What was ruled out:
- Network: <yes/no, with evidence from metrics JSON>
- File/tree fetches: <yes/no, with evidence from scmstore>
- EdenFS health: <output of eden doctor>
- Repo state: <output of sl doctor>

Reproducer (if known):
<Steps to reproduce>
```

Suggest posting to the appropriate Workplace group for SCM client support.
</escalation_template>

<diagnostic_tool_crashes>
**Diagnostic tool itself crashes**

- Symptoms: running `sl rage`, `sl doctor`, `eden doctor`, `sl blackbox`, or other diagnostic commands produces a Python/Rust traceback instead of output
- Classification: read the traceback to determine if it's a **code bug** or an **environment issue**

**Code bug indicators** — the crash is in tool internals, not the user's fault:
- `RuntimeError` with a descriptive message (e.g., `"Should not invoke __len__ on eden_dirstate_map!"`)
- `AssertionError`, `TypeError`, `AttributeError` in internal modules
- `KeyError` / `IndexError` in internal data structure access
- Stack trace points to Sapling/EdenFS source code, not user config or filesystem
- Fix: tell the user this is a known class of tool bug. Collect the **Error ID** if present (`Error Id: xxxxxxxx-xxxx-...`). Direct them to post at https://fb.workplace.com/groups/saplingusers with the full traceback and Error ID. Then help with their original problem using alternative commands.

**Environment issue indicators** — the crash reveals the user's actual problem:
- `PermissionError` — permission issues on repo or config files
- `OSError` with `Errno 28` (disk full), `Errno 70` (stale NFS), `Errno 107` (transport endpoint)
- `FileNotFoundError` — missing repo, config, or backing store
- `ConnectionError` / `TimeoutError` — network issues
- Fix: route to the corresponding diagnostic path (stale mount, disk space, permissions, network)

**Investigate the crash itself:**
- The traceback is diagnostic evidence — analyze it, don't just classify it. The stack frames, file paths, variable names, and error arguments reveal the repo's state and configuration. For example, a crash in `eden_dirstate_map` tells you the repo uses EdenFS-backed dirstate; a crash referencing a specific path tells you which mount or backing store is involved.
- For environment issues, the errno, path, and context in the traceback point directly to the root cause — use them to target your next checks.

**Tool fallbacks when a diagnostic command crashes:**
- `sl rage` crashed → use `eden rage` for EdenFS diagnostics, or collect individual rage sections manually (see [rage-sections.md](rage-sections.md))
- `sl doctor` crashed → run `eden doctor` and individual health checks (`eden status`, `eden list --json`)
- `eden doctor` crashed → run `eden status`, `eden list --json`, `mount | grep <repo>`, manual mount checks
- `sl blackbox` crashed → check `~/.eden/logs/edenfs.log` directly for EdenFS events; for sl events, try `sl blackbox` with a different `--start` range or pattern
</diagnostic_tool_crashes>
