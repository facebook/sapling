---
name: diagnose-sapling
description: |
  Diagnoses slow, hanging, or failing Sapling (sl) commands using blackbox logs, doctor checks, and CLI diagnostics.
  Use when a user reports slow commands, EdenFS issues, checkout failures, or general Sapling/EdenFS problems.
  Generates shell commands for investigation — does not write code or modify internals.
---

<objective>
Diagnose Sapling and EdenFS problems by running diagnostic shell commands and interpreting their output. Does not write code or modify source.
</objective>

<context>
**Problem domains:**
- **Working copy** (files on disk, checkout) — EdenFS, watchman, treestate, disk space, checkout failures, merge drivers, certs, file I/O hangs
- **Commit graph** (history, DAG, sync) — segments/DAG corruption, metalog, visibility heads, cloud sync, pull/push/network, commit/amend/fold

**Key facts:**
- **Client** (Sapling CLI + EdenFS) vs **Server** (Mononoke) — most issues are client-side.
- **ISL** depends on the CLI — ISL problems are usually CLI problems.
- **EdenFS** is a virtual filesystem between the kernel and Sapling's backing store (working copy domain).

**Before running commands:** Use any output the user already provided — don't re-run those commands. If an error isn't covered in references, search the codebase (`fbcode/eden/fs/` and `fbcode/eden/scm/`) for the error string. If multiple issues, address the blocking one first. Categorize based on evidence (e.g., `StaleMountsFound` = mount state, not "eden doctor error"), not just the user's words.

**Tool hierarchy (targeted first, rage only for escalation):**
- `sl blackbox` — event logs, fast, filterable. Use first. **Note:** `--start N` only searches current log file; older events in rotated logs — see [references/blackbox-guide.md](references/blackbox-guide.md).
- `sl doctor` / `eden doctor` — health checks with auto-repair. Run early.
- `sl debugnetwork` / `sl debugnetworkdoctor` — network test. Run when network is suspected.
- `sl rage` / `eden rage` — comprehensive dumps (9,000+ lines). **Escalation only.** For targeted collection, run underlying commands directly per [references/rage-sections.md](references/rage-sections.md).
</context>

<quick_start>
**Route by symptom, then run the listed checks in parallel. Interpret results using the linked reference.**

**1. Command is SLOW**

First, determine: **is it all commands or just one specific command?**
- Ask the user: "Is every `sl` command slow, or just a specific one (e.g., `sl status`)?"
- If **all commands** are slow → suspect system-level issues: disk I/O, EdenFS health, network. Run `eden status`, `df -h`, `eden stats --json 2>/dev/null` first. If local diagnostics aren't conclusive, query Scuba `dev_command_timers` for p50/p95 timing on this host to see if the slowness is consistent or intermittent — see [references/scuba-tables.md](references/scuba-tables.md).
- If **one command** is slow → route by the specific command below.
- If the user doesn't know → `sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'` to find which commands are slow.

`sl status` slow → **run in parallel:**
- `watchman debug-status`
- `eden stats --json 2>/dev/null`
- `sl blackbox --start 60 --pattern '{"watchman": "_"}'`
- `sl blackbox --start 60 --pattern '{"blocked": "_"}'`
- `eden doctor --dry-run`
- Interpret: [references/working-copy.md](references/working-copy.md). If eden stats shows >100K loaded inodes, also see [references/scuba-tables.md](references/scuba-tables.md) to query `edenfs_events` for the culprit.

`sl commit` / `sl amend` slow → **run in parallel:**
- `sl blackbox --start 60 --pattern '{"blocked": "_"}'`
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'`
- `eden stats --json 2>/dev/null`
- `sl debugrunlog --ended`
- Interpret: [references/commit-graph.md](references/commit-graph.md). Blocked events show hook/editor wait time. Check if amend triggered restack of child commits.

`sl log` / `sl smartlog` slow → **run in parallel:**
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 5000, 999999]}}'`
- `sl debugvisibleheads | wc -l`
- `sl debugnetworkdoctor`
- Interpret: [references/commit-graph.md](references/commit-graph.md). 200-300 draft heads is normal; 1000+ is a problem.

`sl checkout` / `sl update` slow → **run in parallel:**
- `eden stats --json 2>/dev/null`
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'`
- `eden trace sl --retroactive`
- `df -h`
- Interpret: [references/working-copy.md](references/working-copy.md). Check fetch counts in blackbox metrics JSON.

`sl rebase` slow → **run in parallel:**
- `sl blackbox --start 60 --pattern '{"blocked": "_"}'`
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'`
- `eden stats --json 2>/dev/null`
- `sl config experimental.mergedriver`
- Interpret: if blocked events show `blockedtag="mergedriver"` → [references/merge-driver.md](references/merge-driver.md). Otherwise → [references/commit-graph.md](references/commit-graph.md).

`sl fold` slow → **run in parallel:**
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'`
- `sl debugrunlog --ended`
- Interpret: [references/commit-graph.md](references/commit-graph.md). Many commits + automatic restack = expected slowness.

`sl pull` / `sl push` slow → **run in parallel:**
- `sl debugnetworkdoctor`
- `sl blackbox --start 60 --pattern '{"finish": {"duration_ms": ["range", 5000, 999999]}}'`
- Interpret: [references/commit-graph.md](references/commit-graph.md). Check EdenAPI timing in blackbox metrics JSON.

**To find slow commands when the user doesn't know which one:** `sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'`

**2. Command is HANGING (seems stuck)**

First, determine: **is it all commands or just one?**
- If another `sl` command works fine → the hang is specific to that command (lock contention, hook, network).
- If **all** `sl` commands hang → suspect EdenFS or system-level issue.

→ **run in parallel:**
- `sl debugrunlog`
- `sl debugprocesstree`
- `sl blackbox --start 60 --pattern '{"blocked": "_"}'`
- `eden status`
- `sl debuglocks`
- Interpret: [references/hang-diagnosis.md](references/hang-diagnosis.md). Check `debugrunlog` download_bytes — if climbing, not hung, just slow. If flat, may be truly stuck.
- **ISL vs terminal**: ISL passes `--config amend.autorestack=always` which forces restack even on conflicts. Check the `[command]` line in blackbox to distinguish.

**If process is still alive but stuck** → use `strace` to see what it's blocked on. See [references/hang-diagnosis.md](references/hang-diagnosis.md) for strace commands and interpretation.

**3. Command FAILS with an error** → **run in parallel:**
- `sl doctor`
- `eden doctor`
- `sl blackbox --start 60`
- Then route by error message:
  - 500 Internal Server Error / "empty common" → [references/commit-graph.md](references/commit-graph.md) (`sl debugrebuildchangelog`)
  - Mount errors / I/O errors → [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md)
  - "nested repo" during rebase → [references/merge-driver.md](references/merge-driver.md) (fix: `rm -rf ~/fbsource/www/.hg`)
  - "Autogenerate failed" during rebase → [references/merge-driver.md](references/merge-driver.md)
  - Certificate / x509 errors → [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) (fix: `update-certificates`)
  - Filter errors / "Failed to get filter" / `UnexpectedMountProblem` → [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) (see "FilteredFS / eden-sparse issues")
  - Treestate / metalog errors → [references/working-copy.md](references/working-copy.md)
  - Cloud sync errors → [references/commit-graph.md](references/commit-graph.md)

**4. Wrong results (missing files, wrong content, lost commits)** → **run in parallel:**
- `eden doctor --dry-run`
- `eden status`
- `sl blackbox --start 1440 --pattern '{"finish": "_"}'`
- Then route:
  - Files missing or wrong on disk → [references/working-copy.md](references/working-copy.md)
  - Commit appears lost → `sl debugmutation -r <hash>` to trace rewrites. See [references/commit-graph.md](references/commit-graph.md).
  - **After rebase: missing lines, wrong content** → likely merge driver. See [references/merge-driver.md](references/merge-driver.md).
  - Cloud sync duplicating commits → [references/commit-graph.md](references/commit-graph.md)

**5. EdenFS issues (mount, memory, disk, crashes, redirections)**

EdenFS not starting / restart stuck → **run in parallel:**
- `eden list --json`
- `eden status`
- `cat ~/.eden/config.json`
- `eden debug log 2>&1 | tail -50`
- `df -h`
- Interpret: [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md). Check which mount is stuck in `eden list`. If a broken mount is blocking startup, remove its entry from `~/.eden/config.json` and restart.

EdenFS high memory / OOM → **run in parallel:**
- `eden stats --json 2>/dev/null`
- `eden list --json`
- `eden redirect list`
- `dmesg | grep -i -E 'oom|eden|killed' | tail -20`
- Interpret: [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) (see "Diagnosing OOM kills"). If loaded inodes >100K, see [references/scuba-tables.md](references/scuba-tables.md).

EdenFS crashed or unhealthy → **run in parallel:**
- `eden status`
- `eden uptime`
- `eden list --json`
- `eden debug log 2>&1 | tail -50`
- `dmesg | grep -i -E 'oom|eden|killed' | tail -20`
- Interpret: [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) (see "Timestamp correlation" for crash investigation).

Disk space issues → **run in parallel:**
- `eden du --fast`
- `df -h`
- `eden redirect list`
- Interpret: [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md)

EdenFS config issues → **run:**
- `edenfsctl fsconfig --all`
- Interpret: [references/eden-config.md](references/eden-config.md)

**If unclear**, or if the error doesn't match any pattern above, collect baseline diagnostic context. These commands work **without a repo**: `eden version`, `sl --version`, `eden status`, `eden list --json`, `edenfsctl fsconfig --all`. For platform-specific package info: `rpm -q fb-eden` (Linux/macOS) or `eden version` (Windows). **If a repo exists**, also run: `sl blackbox --start 60`, `sl debuginstall`, `sl debugdetectissues`. See [references/rage-sections.md](references/rage-sections.md) for the full problem-to-sections mapping.

**6. Diagnostic tool crashes — when `sl`, `eden`, or other tools crash during diagnosis**

If a diagnostic command (`sl rage`, `sl doctor`, `eden doctor`, `sl blackbox`, etc.) itself crashes with a traceback:

1. **Read the traceback.** Classify it:
   - **Code bug** — `RuntimeError`, `AssertionError`, `TypeError`, `AttributeError`, deliberate raises with descriptive messages (e.g., `"Should not invoke __len__"`), or `KeyError`/`IndexError` in internal logic. These are tool bugs, not user problems.
   - **Environment issue** — `PermissionError`, `OSError` (disk full, stale mount), `FileNotFoundError` (missing repo/config), `ConnectionError` (network). These indicate the underlying problem to diagnose.

2. **For code bugs:**
   - Tell the user this is a bug in the tool — not in their repo or environment.
   - Collect the Error ID if the output contains one (e.g., `Error Id: xxxxxxxx-xxxx-...`).
   - Direct them to post at https://fb.workplace.com/groups/saplingusers with the full traceback and Error ID.
   - Ask what they were originally trying to do, then help with alternative commands (skip the crashed tool).

3. **For environment issues:** The crash is a symptom — route to the appropriate section above (e.g., stale mount, disk space, permissions).

4. **Tool fallbacks:** If `sl rage` crashes → `eden rage` for EdenFS info, or collect sections manually per [references/rage-sections.md](references/rage-sections.md). If `sl doctor` crashes → run `eden doctor` and individual checks. If `eden doctor` crashes → try `eden status`, `eden list --json`, manual mount checks.

**7. Second-stage checks — when initial results need deeper investigation**

After interpreting initial check results, if the root cause is identified but not yet actionable, or if the initial fix failed, run targeted follow-up checks:

- **Stale mount / mount won't remount** → `mount | grep <repo_name>` (check if kernel still has stale mount registered), `df -h` (disk full preventing mount?)
- **OOM / crash found** → `dmesg | grep -i -E 'oom|eden|killed'` to confirm. See [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) "Diagnosing OOM kills" for full investigation steps.
- **Hang — process still alive** → `strace` to find what it's blocked on. See [references/hang-diagnosis.md](references/hang-diagnosis.md).
- **High inode count found** → query Scuba `edenfs_events` for `fetch_heavy`/`big_walk` by `client_cmdline` to find the culprit process
- **Doctor fix failed** → read the error output from doctor's fix attempt, determine why the fix failed, try the next fix in the escalation ladder (see references)
- **Disk full found** → `eden du --fast` for EdenFS breakdown, identify what's consuming space (materialized files, backing repos, orphaned redirections)
- **Certificate issue found** → check cert file directly (`ls -la /var/facebook/credentials/$USER/x509/`), try `update-certificates`
</quick_start>

<anti_patterns>
- **NEVER invent or hallucinate commands.** Only suggest commands listed in the reference files, or that you have verified exist by running `<command> --help` first.
- **Do not guess without evidence.** If blackbox doesn't show what caused a slowdown, say so rather than speculating. If you don't know the fix, say so and suggest escalation.
- **Do not claim network is the bottleneck** unless EdenAPI timing in the metrics JSON actually shows significant time relative to total duration.
- **Do not assume high visible head count is a problem.** 200-300 draft heads is normal. Only flag at 1000+.
- **Do not ignore the process tree.** A slow command launched by `devenv-backup` is a different problem than one launched by the user.
- **Do not conflate working copy and commit graph issues.** They have different causes and different fixes.
- **Do not run interactive/TUI commands.** Commands like `sl debugtop`, `eden top`, `eden minitop --interactive`, and `sl cloud sl --history` require a TTY. Use their non-interactive equivalents (`sl debugrunlog`, `eden stats`, etc.) instead.
- **Ask questions when the symptom is ambiguous.** Don't assume which domain the problem is in — ask the user to clarify.
- **NEVER run mutating source control operations without asking the user first.** Commands like `sl goto`, `sl rebase`, `sl pull`, `sl push`, `sl commit`, `sl amend`, `sl revert`, `sl shelve`, `sl unshelve`, `sl hide`, `sl unhide`, `eden restart`, `eden unmount`, `watchman watch-del-all`, and any command that modifies the working copy, commit graph, or repository state require explicit user permission. Read-only diagnostic commands are fine to run without asking.
</anti_patterns>

<success_criteria>
- The problem domain (working copy vs commit graph) has been identified
- The domain-specific diagnostic path has been followed
- Relevant diagnostic commands have been run and their output analyzed
- Each finding is supported by a specific output line
- What is known and what is unknown are clearly separated
- Either a fix has been applied (via doctor, config change) or a structured escalation summary has been prepared
- After applying a fix, verification commands have been run to confirm the fix worked (e.g., `eden status`, `eden list --json`, `ls <mount_path>`, or the originally-failing command)
- If verification fails, the next fix in the escalation ladder has been attempted or escalation has been prepared
- Telemetry has been logged (see `<telemetry>` section below)
</success_criteria>

<telemetry>
**You MUST log telemetry at two points: after triage and at conclusion.** See [references/telemetry.md](references/telemetry.md) for the full template, field definitions, and rules.
</telemetry>

<references>
Deep-dive guides — read these when you need detailed diagnostic steps for a specific area:

- [references/working-copy.md](references/working-copy.md) — Working copy problems: files on disk, file states, watchman, treestate, checkout failures
- [references/edenfs-diagnosis.md](references/edenfs-diagnosis.md) — EdenFS problems: crashes, restarts, redirections, tracing, log analysis, disk space, overlay corruption
- [references/commit-graph.md](references/commit-graph.md) — Commit graph problems: DAG/segments corruption, metalog, visibility heads, cloud sync, commit/amend/fold operations
- [references/merge-driver.md](references/merge-driver.md) — Merge driver and merge tool problems: CIGAR rebuilders, slow rebases, autogenerate failures, silent change loss, merge tool config, tool selection priority, merge-patterns, premerge settings
- [references/hang-diagnosis.md](references/hang-diagnosis.md) — Hanging commands: debugrunlog, debugbacktrace, ISL vs terminal differences, lock contention
- [references/blackbox-guide.md](references/blackbox-guide.md) — Blackbox interpretation: reading events, metrics JSON, rotated logs, finding time gaps
- [references/sapling-config.md](references/sapling-config.md) — Sapling config: 8-level priority hierarchy, debugging with `--verbose --debug`, remote/dynamic config
- [references/eden-config.md](references/eden-config.md) — EdenFS config: 5-level hierarchy, `edenfsctl fsconfig --all`, CLI vs daemon divergence, hang-causing settings
- [references/eden-stats.md](references/eden-stats.md) — `eden stats --json`: inode counts, memory usage, cache hit rates, fetch latency counters
- [references/eden-architecture.md](references/eden-architecture.md) — EdenFS storage architecture: ObjectStore, BackingStore, SaplingBackingStore, fetch pipeline
- [references/scuba-tables.md](references/scuba-tables.md) — Scuba tables: edenfs_events, dev_command_timers, watchman_events, ODS counters, query examples
- [references/escalation.md](references/escalation.md) — Escalation: rage file guidance, support post structure, what to include
- [references/common-issues.md](references/common-issues.md) — Known issues catalog: symptoms, diagnostic evidence, and fixes organized by domain
- [references/rage-sections.md](references/rage-sections.md) — Rage section inventory: maps `sl rage` / `eden rage` sections to problem categories, with underlying commands. Static fallback when rage source files are inaccessible.
- [references/telemetry.md](references/telemetry.md) — Telemetry: scribe_cat template, field definitions, logging rules
</references>
