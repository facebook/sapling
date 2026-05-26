# EdenFS Lifecycle Timeline Builder

Build a chronological timeline of EdenFS lifecycle events to understand how the system reached its current state. This combines Scuba telemetry with local daemon logs to show every restart, upgrade, crash, and recovery.

## When to Use

When the user asks:
- "what happened to edenfs on this host?"
- "show me the edenfs lifecycle timeline"
- "why did edenfs restart?"
- "how did edenfs get into this state?"
- "build a timeline of edenfs events"
- "generate timeline of edenfs PID"

## Procedure

### Step 1: Gather Parameters

Ask the user for:
- **Time range**: e.g., "last 24 hours", "last 7 days", "since May 15". Default to last 24 hours if not specified.
- **Host**: detect with `hostname`. **IMPORTANT: Strip the `.facebook.com` suffix** — Scuba uses the short form (e.g., `devvm21611.cco0`, not `devvm21611.cco0.facebook.com`). Use: `hostname | sed 's/\.facebook\.com$//'`
- **User**: detect with `whoami`. If triaging a remote host, ask.

### Step 2: Collect Events from All Sources

Collect from three sources **in parallel**: a Scuba SQL query for all lifecycle event types, local eden debug log grep commands, and systemd service properties.

#### Source 1: Single Scuba SQL Query (all lifecycle events)

Use a **single SQL query** with `IN (...)` to fetch all lifecycle event types at once. This is much faster than running separate queries per type.

**All lifecycle event types in edenfs_events:**

| Type | What it captures |
|------|-----------------|
| `edenfs_upgrade` | Hourly `edenfs_restarter` runs — upgrade decisions |
| `systemctl_action` | `systemctl --user start/reload` commands sent to systemd |
| `systemd_start` | Daemon startup attempts (logged by `edenfsctl systemd-start`) |
| `daemon_start` | Daemon successfully started and running |
| `daemon_stop` | Daemon stopped (graceful or forced) |
| `graceful_restart` | Graceful restart attempt outcome (success/failure) |
| `silent_daemon_exit` | Daemon exited without explicit stop — potential crash or signal |

Convert the requested time range to seconds for the SQL `WHERE time >= NOW()-<seconds>` clause:
- 1 day = 86400, 7 days = 604800, 14 days = 1209600

```bash
meta scuba.dataset query -d edenfs_events --sql "SELECT time, FROM_UNIXTIME(time, 'America/Los_Angeles') AS ts, type, action, success, error, exit_code, exit_signal, pid, session_id, edenver, is_takeover, args_file_age_s, mode, num_restarts FROM edenfs_events WHERE time >= NOW()-<SECONDS> AND host = '<SHORT_HOST>' AND user = '<USER>' AND type IN ('systemctl_action', 'systemd_start', 'daemon_start', 'daemon_stop', 'graceful_restart', 'silent_daemon_exit', 'edenfs_upgrade') ORDER BY time ASC LIMIT 500" --output=toon -r "EdenFS lifecycle timeline for <USER>@<HOST>"
```

**Key fields by event type:**

`edenfs_upgrade`:
- `mode`: "normal", "graceful", or "idle_delay" (deferred upgrade)
- `num_restarts`: null/0 means no upgrade needed
- `success`: 1 = upgrade succeeded, 0 = failed

`systemctl_action`:
- `action`: "start" (eden start / force restart) or "reload" (graceful restart)
- `args_file_age_s`: **critical for distinguishing restart types** — 0 = intentional (fresh args), >3600 = auto-restart by systemd (reusing old args)
- `is_takeover`: whether this was a graceful restart with FUSE takeover

`daemon_start` / `daemon_stop`:
- `session_id`: links start/stop pairs for the same daemon instance
- `edenver`: build version — "(dev build)" for locally-built binaries
- `is_takeover`: 1 = graceful handoff, 0 = fresh start or hard stop

`graceful_restart`:
- `success`: 0 with `error` field explains failure (e.g., "Graceful restart failed, and old EdenFS process resumed")

`silent_daemon_exit`:
- `exit_signal`: 0 = clean exit, non-zero = signal (SIGTERM=15, SIGKILL=9, SIGABRT=6)

#### Source 2: Local eden debug log

The daemon's own log provides PIDs and confirms startup completion. Run these **in parallel**:

```bash
eden debug log | grep "Starting edenfs" | tail -20
eden debug log | grep "Started EdenFS" | tail -20
eden debug log | grep -E "SIGABRT|SIGSEGV|signal|abort|fatal" | tail -20
eden debug log | grep -i "takeover\|gracefully transfer" | tail -20
eden debug log | grep -i "shutting down\|initiateShutdown\|performCleanup" | tail -20
```

Parse timestamps and PIDs:
- Start: `V<MMDD> <HH:MM:SS.micros> <PID> StartupLogger.cpp:NNN] Starting edenfs <version>, pid <PID>, session_id <SID>`
- Complete: `I<MMDD> <HH:MM:SS.micros> <PID> StartupLogger.cpp:NNN] Started EdenFS (pid <PID>, session_id <SID>) in <N>s`

#### Source 3: Systemd Service Properties

Collect comprehensive systemd properties for each EdenFS instance. These provide the restart policy, crash recovery counters, and — critically — distinguish when the **unit** became active vs when the **current process** started (which differ across graceful restarts).

```bash
# Main instance
systemctl --user show edenfs@home-$(whoami)-local-.eden.service \
  --property=Id,Type,Restart,RestartUSec,StartLimitIntervalUSec,StartLimitBurst,NRestarts,ExecMainStartTimestamp,ExecMainPID,ExecMainCode,ExecMainStatus,ActiveState,SubState,Result,InvocationID,ActiveEnterTimestamp,ActiveExitTimestamp,InactiveEnterTimestamp,InactiveExitTimestamp
```

**Key properties to present:**

| Property | Why it matters |
|----------|---------------|
| `Restart` | Restart policy (e.g., `on-failure`) — determines what triggers auto-restarts |
| `RestartUSec` | Delay before auto-restart attempt |
| `StartLimitBurst` / `StartLimitIntervalUSec` | Crash loop protection — e.g., 5 restarts in 2 min = service enters `failed` |
| `NRestarts` | Number of crash-triggered auto-restarts in the current activation cycle |
| `ActiveEnterTimestamp` | When the **unit** became active — persists across `reload` (graceful restart) |
| `ExecMainStartTimestamp` | When the **current process** was forked — resets on each `reload` |
| `ExecMainPID` | Current main process PID |
| `Result` | `success` or the failure reason for the last stop |
| `Type` | `notify` means edenfs uses `sd_notify(READY=1)` to signal readiness |
| `InvocationID` | Unique ID for this activation cycle |

**`ActiveEnterTimestamp` vs `ExecMainStartTimestamp`**: These often differ significantly. Graceful restarts use `systemctl reload`, which replaces the main process without leaving the `active` state. Systemd considers the unit continuously active since the last `start`, even though the binary may have been replaced multiple times via `reload`. Always present both timestamps to show this distinction.

#### Source 4: Systemd Journal (if accessible)

The systemd journal shows service state transition messages (`Started`, `Stopping`, `Main process exited`). Access may be restricted on some hosts.

```bash
journalctl --user -u 'edenfs@home-<USER>-local-.eden.service' --since '<DATE>' --no-pager -q 2>&1
```

If this returns "No journal files were opened due to insufficient permissions", note the limitation in the timeline output — the user isn't in the `systemd-journal` group. The Scuba + `systemctl show` data is sufficient to reconstruct the timeline, but journal entries provide additional context for failure diagnostics.

### Step 3: Build the Timeline

Merge Scuba events and local log entries into a single chronological list. Tag each:
- `[scuba]` — Scuba telemetry events
- `[eden-log]` — local daemon log entries
- `[systemd]` — systemd service properties (NRestarts, etc.)
- `[upgrade-log]` — edenfs_upgrade log file entries

### Step 4: Separate Multiple EdenFS Instances

**A host may run multiple EdenFS instances** with different state directories (e.g., main at `/home/<user>/local/.eden` and a dev instance at `/home/<user>/eden-dev-state`). Scuba events from all instances are intermixed.

How to distinguish them:
- **Error messages** from failed `systemctl_action` events include the systemd unit name (e.g., `edenfs@home-lxw-eden\\x2ddev\\x2dstate.service` vs `edenfs@home-lxw-local-.eden.service`)
- **`edenver` field**: `(dev build)` indicates a locally-built binary, typically used on a dev-state instance
- **`session_id`**: track session IDs to link start/stop pairs within the same instance

Present dev-state events separately or as a collapsed summary to avoid cluttering the main timeline.

### Step 5: Present the Timeline

Format as a table. Show a PID summary table first, then a systemd properties section, then detailed events grouped by related sequences.

**PID summary table:**

```
PID         Build               Session ID   Active Period
─────────── ──────────────────── ──────────── ──────────────────────────────
27818       20260504-072502      1536670623   ... → May 11 23:15:52
3220807     20260511-184259      27991651     May 11 23:15:55 → May 12 10:12:12
1249109     20260512-015432      672130067    May 12 10:12:15 → May 15 13:35:38
71898       20260515-062901      2613755311   May 15 13:45:05 → now (7 days)
```

**Systemd service properties** — present as a dedicated table section between the PID summary and detailed events. Include `Restart`, `NRestarts`, `StartLimitBurst`/`StartLimitIntervalUSec`, `ActiveEnterTimestamp`, `ExecMainStartTimestamp`, `ExecMainPID`, `Result`, `Type`, and `InvocationID`. When `ActiveEnterTimestamp` and `ExecMainStartTimestamp` differ, call out that graceful restarts (`reload`) replaced the process without resetting the unit's active state.

**Detailed events** — group related events (upgrade → reload → stop → start) visually with separator lines.

### Step 6: Annotate with Interpretation

After the timeline, add a brief analysis:
- How many restarts occurred and what triggered them (upgrade vs crash vs manual)
- Whether any restarts failed and why
- The current state and whether it's healthy
- If `args_file_age_s` is large on a `systemctl_action` start event, flag it as an **automatic systemd restart** (crash recovery), not an intentional user/upgrade restart

### Distinguishing Restart Types

| Trigger | How to identify |
|---------|----------------|
| **Manual** (`eden start`/`eden restart`) | `systemctl_action` with small `args_file_age_s` (<60s), no preceding `edenfs_upgrade` event |
| **Upgrade** (edenfs_restarter) | `edenfs_upgrade` event immediately preceding the `systemctl_action`, mode=graceful or normal |
| **Auto-restart** (crash recovery) | `systemctl_action` with large `args_file_age_s` (>3600s), systemd reusing old args file. Often preceded by a crash in eden-log |
| **Graceful restart** | `systemctl_action` with `action=reload`, `is_takeover=true` |
| **Idle delay** | `edenfs_upgrade` with `mode=idle_delay` — restarter decided to defer |
