# Scuba Tables for Diagnosis

**Scuba tables that EdenFS, Sapling, and Watchman log to. Use these for investigation when local diagnostics (blackbox, eden stats, eden log) aren't sufficient.**

## How to query from fbcode

Use the `jf graphql` API (the `phps` ScriptControllers are for www only):

```bash
# Execute a query
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "TABLE_NAME",
  metric: "COUNT",
  group_by: ["COLUMN"],
  hours: 24,
  constraints_json: "[{\"column\":\"COL\",\"op\":\"eq\",\"values\":[\"VAL\"]}]"
}) { success scuba_url results error } } }'

# Get column info for a table
jf graphql --query '{ xfb_scuba_tools { dataset_info(input: {
  dataset: "TABLE_NAME",
  query: "list all columns",
  include_all_columns: true
}) { success dataset_info { columns { column_name column_type } } error } } }'

# Get possible values for a column
jf graphql --query '{ xfb_scuba_tools { column_values(input: {
  dataset: "TABLE_NAME",
  column: "COLUMN",
  execute: true,
  limit: 30
}) { success column_values error } } }'
```

The `scuba_url` in the response gives a clickable link the user can open in their browser.

---

## edenfs_events

**What:** High-signal events from EdenFS CLI and daemon. This is the primary EdenFS diagnostic table.

**When to query:**
- `eden stats --json` shows >1M loaded inodes → find which tool caused it
- Slow EdenFS operations → check checkout timing, long-running FS requests
- EdenFS crashes/restarts → check daemon_start/daemon_stop events
- Heavy fetches seen in eden log → quantify by client command line

**Key event types** (column: `type`):

| type | What it means | Key columns to use |
|------|---------------|-------------------|
| `fetch_heavy` | A process is doing heavy fetches through EdenFS | `client_cmdline`, `fetch_count`, `mount`, `host` |
| `big_walk` | Large directory traversal (IDE, build system) | `client_cmdline`, `walk_duration`, `walk_depth`, `loaded_inodes` |
| `health-report` | Periodic health snapshot | `loaded_inodes`, `unloaded_inodes`, `mount` |
| `checkout` | Checkout/update performance | `duration_checkout`, `duration_diff`, `duration_lookup_trees`, `num_conflicts` |
| `long_running_fs_request` | Slow FUSE/NFS operation | `duration`, `method`, `client_cmdline` |
| `working_copy_gc` | Garbage collection performance | `duration`, `loaded_inodes`, `num_deleted_inodes` |
| `daemon_start` | EdenFS started | `duration`, `is_takeover`, `force_restart`, `host` |
| `daemon_stop` | EdenFS stopped | `host`, `exit_signal` |
| `eden_doctor` | Doctor run results | `num_problems`, `num_fixed_problems`, `problems` |
| `eden_doctor_dry_run` | Doctor dry-run results | `num_detected_issues`, `detected_issues` |
| `mount` | Mount event | `mount`, `duration`, `error` |
| `fetch_miss` | Cache miss causing network fetch | `fetched_object_type`, `miss_type`, `fetch_mode` |
| `accidental_unmount_recovery` | Accidental unmount detected and recovered | `mount`, `success` |
| `inode_loading_failed` | Inode failed to load | `mount`, `load_error` |
| `expensive_glob` | Expensive glob operation | `glob_request`, `duration` |

**Common diagnostic queries:**

```bash
# Which tool is causing heavy fetches on this host (last 24h)?
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "edenfs_events",
  metric: "COUNT",
  group_by: ["client_cmdline"],
  hours: 24,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"type\",\"op\":\"in\",\"values\":[\"fetch_heavy\",\"big_walk\"]}]"
}) { success scuba_url results error } } }'

# EdenFS restart history for this host (last 7 days)
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "edenfs_events",
  metric: "COUNT",
  group_by: ["type"],
  hours: 168,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"type\",\"op\":\"in\",\"values\":[\"daemon_start\",\"daemon_stop\"]}]"
}) { success scuba_url results error } } }'

# Slow checkout operations on this host
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "edenfs_events",
  metric: "p50",
  columns: ["duration_checkout"],
  group_by: ["mount"],
  hours: 24,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"type\",\"op\":\"eq\",\"values\":[\"checkout\"]}]"
}) { success scuba_url results error } } }'

# What doctor problems were found on this host?
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "edenfs_events",
  metric: "COUNT",
  group_by: ["problems"],
  hours: 168,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"type\",\"op\":\"in\",\"values\":[\"eden_doctor\",\"eden_doctor_dry_run\"]}]"
}) { success scuba_url results error } } }'
```

**Caveats:**
- Hostname for On Demand instances is the OD task name (e.g., `44717.od.fbinfra.net`), not the parent physical hostname
- Events are logged from both CLI and daemon — use the `type` column to distinguish

---

## edenfs_cli_usage

**What:** Every `edenfsctl` command invocation. The EdenFS equivalent of `dev_command_timers`.

**When to query:**
- Want to see what eden commands a user ran and whether they succeeded
- Investigating timing of specific eden CLI operations
- Correlating eden CLI calls with edenfs_events

**Key columns:**
- `time` — when the command was issued
- `success` — whether it succeeded
- `arguments` — command arguments
- `user` — who ran it
- `host` — where it ran
- `os` — platform

**Limitations:**
- Cannot log stdout/stderr of Python commands
- For additional detail, join with `edenfs_events`

---

## dev_command_timers

**What:** Every `sl`/`hg`/`arc`/`git` command invocation. The primary Sapling performance table.

**When to query:**
- Investigating slow Sapling commands beyond what blackbox shows
- Want aggregate performance data (p50, p95) for a command type
- Need error backtraces
- Comparing EdenFS vs non-EdenFS performance

**Key columns:**

| Column | What it tells you |
|--------|-------------------|
| `command` | Which sl/hg command was run |
| `logged_by` | Which tool (`hg`, `sl`) — filter to avoid arc/git noise |
| `sapling_time` | Command duration in ms **excluding user wait time** (the real perf metric) |
| `edenclientcheckout_time` | Time spent in EdenFS during update/rebase |
| `edenclientstatus_time` | Time spent in EdenFS during status |
| `is_eden` | Whether running on an EdenFS repo |
| `repo` | Which repo |
| `host` | Machine name |
| `unix_username` | Who ran it |
| `operating_system` | OS |
| `errortracekey` | Link to backtrace in Manifold (sometimes available) |
| `is_filteredfs` | Whether FilteredFS is active |
| `active_sparse_profile` | Which sparse profile is active |
| `working directory parent before/after` | Commit hashes before/after — useful for finding lost commits |

**Common diagnostic queries:**

```bash
# What commands did a user run in the last 24h?
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "dev_command_timers",
  metric: "COUNT",
  group_by: ["command"],
  hours: 24,
  constraints_json: "[{\"column\":\"unix_username\",\"op\":\"eq\",\"values\":[\"USERNAME\"]},{\"column\":\"logged_by\",\"op\":\"in\",\"values\":[\"hg\",\"sl\"]}]"
}) { success scuba_url results error } } }'

# P50 sapling_time for sl status on this host
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "dev_command_timers",
  metric: "p50",
  columns: ["sapling_time"],
  group_by: ["command"],
  hours: 168,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"command\",\"op\":\"eq\",\"values\":[\"status\"]},{\"column\":\"logged_by\",\"op\":\"in\",\"values\":[\"hg\",\"sl\"]}]"
}) { success scuba_url results error } } }'

# How much time is EdenFS contributing to sl status?
jf graphql --query '{ xfb_scuba_tools { execute_query(input: {
  dataset: "dev_command_timers",
  metric: "p50",
  columns: ["edenclientstatus_time", "sapling_time"],
  hours: 24,
  constraints_json: "[{\"column\":\"host\",\"op\":\"eq\",\"values\":[\"HOSTNAME\"]},{\"column\":\"command\",\"op\":\"eq\",\"values\":[\"status\"]},{\"column\":\"is_eden\",\"op\":\"eq\",\"values\":[\"1\"]}]"
}) { success scuba_url results error } } }'
```

---

## watchman_events

**What:** Watchman operations — command dispatch, full crawls, sync-to-now, dropped events, saved state.

**When to query:**
- `sl status` is slow and watchman is suspected
- Watchman did a full crawl (very expensive, causes slow status)
- Watchman lost sync with filesystem events

**Key event types:**
- `dispatch_command` — command received from client (includes `command`, `duration`, `client_pid`)
- `full_crawl` — watchman performed a full directory crawl (very expensive)
- `sync_to_now` — sync flush (includes `success`, `timeoutms`)
- `dropped` — watchman lost sync with fsevents (includes `isKernel`)
- `query_execute` — query execution (includes `results`, `walked`, `fresh_instance`)
- `age_out` — old nodes detected (includes `walked`, `files`, `dirs`)

**Note:** Events are sampled — commands >0.2s, all errors, and every 100th event. There's an `event_count` column to track totals.

---

## edenfs_rollouts

**What:** Which config rollouts a machine is part of. Minimal data — meant to be joined with other tables.

**When to query:**
- Suspect a config rollout is causing issues on a specific machine
- Need to check if a machine is in an experimental rollout

**Key columns:** `username`, `hostname`, `rollout_name`

**Caveat:** On Sandcastle, rollouts are logged to the `sandcastle` table instead.

---

## ODS counters (not Scuba, but related)

EdenFS exports counters to ODS that can be viewed in the ODS UI or queried from the daemon:

```bash
# Query specific counters from the running daemon
eden debug thrift getRegexCounters "^store.sapling.*" --json
eden debug thrift getRegexCounters "^inodemap.*" --json
eden debug thrift getRegexCounters "^scmstore.*" --json

# Get all counters
eden debug thrift getCounters --json
```

**Key counter families:**
- `scmstore.{file|tree}.fetch.{LOCATION}.{cache|local}.{keys|hits|misses|requests|errors}` — Sapling fetch stats exposed through EdenFS. LOCATION: `edenapi`, `aux`, `indexedlog`, `lfs`, `contentstore`. `keys` = actual files/dirs, `requests` = number of batches.
- `store.sapling.get_tree.*` / `store.sapling.get_blob.*` — backing store latency
- `object_store.get_tree.memory_cache.*` — in-memory cache stats
- `fuse.*_us.*` — FUSE operation latency percentiles
- `inodemap.*` — inode tracking stats

**ODS update frequency:** Counters are pushed to ODS every 15 minutes. Values may be stale between pushes.

**Unidash:** `unidash edenfs` (or https://www.internalfb.com/intern/unidash/dashboard/edenfs) — dashboards for crashes, utilization, versions, memory, CLI perf, fetching, config manager health, per-machine investigation.

---

## Diagnostic workflow with Scuba

### Narrow → broaden pattern for dev_command_timers

When investigating a slow command, progressively widen the scope to determine if the problem is local or widespread:

1. **This user, this host** — query `dev_command_timers` for the user's commands on their host. Check `sapling_time` and `edenclientstatus_time`/`edenclientcheckout_time`.
2. **This command, this host, all users** — remove the username filter. Are other users on the same machine also slow? If yes → machine-level issue (disk, EdenFS, config).
3. **This command, all hosts** — remove the host filter. Is the same command slow everywhere? If yes → systemic issue (regression in Sapling or EdenFS, server-side problem).
4. **Time series** — use `view: "time_series"` with `time_bucket: "1 day"` to see when the regression started. Correlate with version changes or rollouts.

**If the problem is widespread (step 3 shows elevated p50/p95 across many hosts):**
- This is NOT a local issue — don't waste time on local diagnosis
- Suggest the user file a post to the Source Control Support group or the relevant oncall
- Include the Scuba URL from your query as evidence
- Check `edenfs_rollouts` to see if affected machines share a rollout

**If the problem is isolated (only this user/host):**
- Continue local investigation: config, watchman health, EdenFS health, disk space

### General slow command investigation

1. Start with local tools: `sl blackbox`, `eden stats --json`, `eden log`
2. If local tools show the problem but not the cause, go to Scuba
3. Query `dev_command_timers` using the narrow → broaden pattern above
4. If EdenFS is slow, query `edenfs_events` for `fetch_heavy`/`big_walk`/`long_running_fs_request` on that host
5. If watchman is slow, query `watchman_events` for `full_crawl` or `dropped` events
6. Share the `scuba_url` from the query response with the user or in the escalation post
