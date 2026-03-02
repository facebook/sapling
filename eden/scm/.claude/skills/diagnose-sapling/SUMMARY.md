# diagnose-sapling Skill Summary

## What it is
Interactive diagnostic skill for Sapling (sl) and EdenFS problems. First-line troubleshooter that runs commands, interprets output, and resolves or escalates.

## Can do
- Run read-only diagnostic commands without asking (`sl blackbox`, `eden doctor`, `sl config --debug`, `sl debugrunlog`, `eden trace`, etc.)
- Interpret blackbox logs — event patterns, PID correlation, metrics JSON (EdenAPI timing, scmstore fetch stats), time gap detection
- Route symptoms to the right diagnostic path via a decision tree
- Debug sapling config — knows priority order (CLI > repo > user > system > dynamic > builtin), how to trace overrides with `sl config --verbose --debug`
- Understand EdenFS storage architecture — the 4-layer cache hierarchy (memory → LocalStore/RocksDB → .hg/store/indexedlog → Mononoke), what each layer stores, why misses happen
- Search source code and internal docs when diagnostics alone don't explain the behavior (has access to `meta:code_search`, `meta:knowledge_search`, `Read`, all standard Claude Code tools)
- Generate structured escalation posts with rage files, environment info, and what was ruled out
- Log telemetry to Scuba via two-phase logging (triage + final) and a PostToolUse hook for core identifiers

## Cannot do
- Write code, scripts, or modify source
- Run mutating operations without user permission (`sl goto`, `sl rebase`, `sl pull`, `sl push`, `sl commit`, `eden restart`, `watchman watch-del-all`, etc.)
- Guess or hallucinate commands — only uses commands listed in the skill or verified via `--help`
- Speculate without evidence — if blackbox doesn't explain it, says so
- Detect session inactivity or abandonment — no background timer or idle hooks; mitigated by two-phase telemetry (triage entry logged early)
- Run in the background or as a parallel agent

## Problem domains

- **Working copy** — EdenFS health, watchman/fsmonitor, treestate corruption, disk space, checkout failures, merge drivers/CIGAR, cert issues, command hangs
- **Commit graph** — DAG/segments corruption, metalog, visibility heads, cloud sync, pull/push/network, mutation tracing
- **EdenFS** — crashes, restarts, mounts, logs, GC, memory/OOM, overlay corruption (fsck), redirections, tracing (`eden trace fs/sl/inode/thrift/task`)
- **Merge drivers** — preprocess/conclude phases, SimpleBuilder/CIGAR, config (`experimental.mergedriver`), bypass, content loss after rebase
- **Network** — EdenAPI timing analysis from blackbox metrics JSON, `sl debugnetwork`, cert issues
- **Disk space** — `eden du --fast`, backing repos, materialized files, orphaned redirections
- **Hangs** — `sl debugrunlog` for running processes, `sl debugbacktrace <PID>`, lock contention, editor/hook blocking
- **Config** — priority order, debugging overrides, dynamic/remote configerator, EdenFS config interaction

## How it routes symptoms
- Slow `sl status` → working copy (watchman/EdenFS)
- Slow `sl log`/`sl smartlog` → commit graph (visibility heads, network)
- Slow `sl rebase` → check for merge driver blocked events first, then commit graph
- I/O errors → EdenFS (eden doctor)
- 500 Internal Server Error → corrupt commit graph (`sl debugrebuildchangelog`)
- Missing commits → mutation tracing (`sl debugmutation`)
- Missing files on disk → eden doctor, overlay health
- Explicitly separates working copy vs commit graph — different causes, different fixes

## Architecture knowledge

### Storage hierarchy (lookup order on cache miss)
1. **Memory cache** — in EdenFS process, nanoseconds, per-mount
2. **LocalStore (RocksDB)** — at `~/.eden/storage/<repo>/rocks-db/`, shared across all mounts of the same repo, microseconds, LRU eviction
3. **BackingStore (.hg/store/)** — Sapling's indexedlog format, per-checkout, milliseconds, never auto-evicted
4. **Mononoke (network)** — source of truth, EdenAPI over HTTP, 100ms+

### LocalStore column families
- `blob` — file contents (ephemeral)
- `tree` — directory listings (ephemeral)
- `blobmeta` / `treemeta` — metadata with hashes (ephemeral)
- `hgproxyhash` — EdenFS ↔ Sapling hash mapping (**persistent, never evicted**)
- `hgcommit2tree` — commit → root tree (ephemeral)

### Sapling .hg/store/ directories
- `indexedlogdatastore/` — file content blobs (zstd compressed)
- `manifests/` — tree objects
- `hgcommits/` — commit metadata and DAG segments
- `metalog/` — bookmarks, visibility
- `mutation/` — commit rewrite history (amend, rebase, fold)

### Why two disk caches
- LocalStore (RocksDB) — hot cache for EdenFS, shared across mounts
- .hg/store/ (indexedlog) — Sapling's native format for version control operations

### Key code locations
- `eden/fs/store/` — LocalStore, BackingStore, KeySpace definitions
- `eden/scm/lib/indexedlog/src/` — indexedlog implementation
- `eden/scm/lib/revisionstore/` — Sapling store layer
- `eden/scm/lib/edenapi/` — network protocol (EdenAPI)
- `eden/scm/lib/config/loader/src/` — config loading

## Config knowledge
- Knows the full config priority chain (8 levels)
- Knows debugging commands (`sl config --verbose --debug`, `sl debugconfigtree`, `sl configfile`)
- Understands EdenFS/Sapling config interaction (EdenFS reloads config every 5 minutes)
- Knows about dynamic/remote config from configerator
- Knows merge driver config (`experimental.mergedriver`) and bypass
- General principle: if blackbox and doctor don't explain the behavior, check config next

## Telemetry

### Two mechanisms
- **PostToolUse hook** (`hooks/log_usage.py`) — fires automatically on skill invocation, logs core identifiers (session_id, unixname, hostname, os_type)
- **SKILL.md scribe_cat** — model calls `scribe_cat` directly with rich diagnostic columns

### Two-phase logging
- **Triage** (`phase: "triage"`, `outcome: "in_progress"`) — logged early after identifying problem domain and symptom. Captures partial data even if session is abandoned.
- **Final** (`phase: "final"`, `outcome: "resolved"/"escalated"/"abandoned"`) — logged at conclusion with full diagnosis details.

### Multi-issue handling
- If user raises a different issue mid-session, model closes current issue (final entry), increments `diagnosis_number`, starts new triage cycle

### Key Scuba columns
- `user_report` — user's own description of the problem
- `tool_finding` — what the tool actually observed (independent of user's report)
- `sl_command` — the specific command involved (if any)
- `problem_domain` — working_copy, commit_graph, edenfs, network, hang, merge_driver, unknown
- `symptom_category` — slow, hanging, error, wrong_results, edenfs_issue, disk_space, unknown
- `diagnosis_summary` — tool's 2-3 sentence assessment
- `findings` — comma-separated structured tags (e.g., `stale_watchman,eden_mount_unhealthy`)
- `outcome` — in_progress, resolved, escalated, abandoned, partial
- `resolution_type` — what fixed it (sl_doctor, eden_doctor, watchman_restart, etc.)
- `root_cause` — identified cause (stale_watchman, corrupt_metalog, disk_full, etc.)
- `phase` — triage or final
- `diagnosis_number` — which issue in the session (1, 2, 3...)

### Scuba table
- Table: `diagnose_sapling_skill`
- Scribe category: `perfpipe_diagnose_sapling_skill`

## Safety guardrails
- Never writes code or modifies source
- Never runs mutating source control operations without explicit user permission
- Never invents or hallucinate commands
- Never speculates without evidence from diagnostic output
- Never conflates working copy and commit graph issues
- Asks questions when the symptom is ambiguous

## Reference materials
- `references/common-issues.md` — known issues catalog organized by domain
- `<sapling_config>` section — config debugging, merge driver config, configerator
- `<eden_architecture>` section — storage hierarchy, LocalStore, .hg/store, code locations
- `<rage_guidance>` section — which rage sections to read per problem domain
- `<blackbox_commands>` section — blackbox patterns, PID correlation, metrics JSON interpretation
