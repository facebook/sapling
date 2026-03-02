# Hang Diagnosis

**Hanging commands — commands that seem stuck, ISL vs terminal differences**

## Check what is currently running

```bash
# Show all currently running sl commands with progress info
sl debugrunlog

# Show recently finished commands (kept for ~1 hour)
sl debugrunlog --ended

# JSON output for scripting
sl debugrunlog -T json
```

`sl debugrunlog` is a real-time view of all running `sl` commands on this repo. Each running `sl` command writes a JSON entry to `.hg/runlog/` and holds a lock file. A background thread updates the entry every 0.5 seconds with:

- **`download_bytes` / `upload_bytes`** — cumulative HTTP bytes transferred. **This is the key signal**: if bytes are climbing, the command is actively fetching from Mononoke (not hung, just slow). If bytes are flat and the command has been running a long time, it may be truly stuck or doing slow local work.
- **`progress`** — active progress bars showing topic, unit, position, and total (e.g., "fetching files 450/1200"). Shows exactly what phase the command is in.
- **`exit_code` / `end_time`** — `None` while running. With `--ended`, you can see recently completed commands (cleaned up after ~1 hour).

**Use cases:**
- **Is the command stuck or making progress?** — run `sl debugrunlog` twice with a few seconds gap and compare `download_bytes` and `progress.position`.
- **Is another sl command holding the repo lock?** — `debugrunlog` shows all concurrent running commands. Lock contention causes hangs.
- **How far along is a long operation?** — progress bars show position/total.
- **What just finished?** — `sl debugrunlog --ended` shows recent commands with their exit codes.

## Check the process tree

```bash
# Show process tree for all sl-related processes
sl debugprocesstree

# Show process tree for a specific PID
sl debugprocesstree <PID>
```

- `debugprocesstree` requires osquery to be installed
- Note: `sl debugtop` is interactive/TUI only — do not run from this skill. Use `sl debugrunlog` instead for programmatic diagnosis.

## Get a backtrace for a stuck command

If a command is truly stuck (not making progress):
```bash
sl debugbacktrace <PID>
```
Extracts a stack trace from the running sl process to see where it's stuck.

## Check blackbox for blocked events

```bash
sl blackbox --start 60 --pattern '{"blocked": "_"}'
```
Shows time spent waiting on: editors (commit message), hooks (pre-commit, post-commit), locks, user input. A command "hanging" may actually be waiting on a hook or editor that hasn't returned.

## Common hang causes

- **Waiting on editor** — the command launched `$EDITOR` for a commit message and is waiting for it to close
- **Waiting on pre-commit hook** — a lint/format hook is running. Check `sl blackbox` for `blocked` events.
- **Lock contention** — another sl command holds the repo lock. Check `sl debugrunlog` for concurrent commands.
- **Network hang** — a fetch or push is stuck waiting on Mononoke. Check `sl debugrunlog` for download/upload bytes not changing. Try `sl debugnetwork` to test connectivity.
- **EdenFS hang** — EdenFS is not responding to file operations. Check `eden status` and `eden doctor`.

## ISL vs terminal commands

ISL (Interactive Smartlog) launches Sapling commands with distinctive flags that override repo config. Recognizing ISL-launched commands is critical because they behave differently from terminal commands.

### How to identify ISL commands in blackbox

ISL commands have these telltale signs in the `[command]` line:
- `--config amend.autorestack=always` — forces restack even on conflicts
- `--config progress.renderer=nodeipc` — ISL's progress reporting
- `--noninteractive` — suppresses prompts
- `--addremove` — auto-adds untracked files

Example ISL amend:
```
[command] ["hg.real", "--config", "amend.autorestack=always", "amend", "--addremove", "--noninteractive", "--config", "progress.renderer=nodeipc"]
```

Example terminal amend:
```
[command]> amend
```

The `[process_tree]` line also confirms — ISL commands show a VSCode/node parent:
```
[process_tree] ... /usr/local/fbpkg/vscodefb/vscode-server/377/node (1865595) -> /usr/local/bin/sl (2713828) -> (this process)
```

### Why this matters for diagnosis

- **`amend.autorestack`**: ISL sets `always`, terminal uses repo default (usually `no-conflict`). With `always`, amend can fail with "unresolved conflicts" during restack. With `no-conflict`, restack is silently skipped.
- **Config overrides**: ISL's `--config` flags take priority over all config files. Running `sl config amend.autorestack` from the terminal shows the repo default, NOT what ISL uses.
- **Cloud sync**: ISL triggers `cloud sync --best-effort --reason amend` after amend. This adds 5-7 seconds and can make the command appear hung.

**`amend.autorestack` values:**
- `always` — restack child commits unconditionally; fail with conflicts if they arise (ISL default via --config)
- `no-conflict` — restack only if no conflicts; silently skip if conflicts exist (typical repo default)
- `never` (or empty) — never auto-restack

**Post-amend cloud sync as perceived hang:**

After a successful amend, Sapling spawns `cloud sync --best-effort --reason amend` as a child process. This typically takes 5-7 seconds. If the user is watching the terminal, the amend appears to "hang" for this duration even though the amend itself completed. Look for the cloud sync PID in blackbox:
```
[command]> cloud sync --best-effort --reason amend
[command_finish]> cloud sync ... exited 0 after 5.82 seconds
```
This is normal behavior, not a hang.

## Stuck background processes

When a user reports high CPU, resource exhaustion, or system sluggishness, the issue may not be a hanging foreground command — it could be background processes consuming resources.

### Investigate

```bash
# Find source-control-related processes using excessive CPU or memory
ps aux --sort=-%cpu | head -20
ps aux --sort=-%mem | head -20

# Count instances of a suspicious process
ps aux | grep <process_name> | wc -l
```

Look for any source-control-related process that is consuming excessive CPU/memory or has accumulated many instances.

### If the process is still alive — strace it

```bash
strace -p <PID> -e trace=network,write -f -tt 2>&1 | head -50
```

This reveals what the process is blocked on — network write, file lock, broken pipe, etc. The strace output tells you whether the process has a transient issue (kill it and move on) or is hitting a bug that needs escalation.

### If the process already exited — use logs

If the problematic process is no longer running, rely on:
- `sl blackbox --start 1440` — find the command's start, finish, duration, and exit code
- Scuba `dev_command_timers` — check invocation counts, timing, error traces for that user/host
- `eden debug log` — if the process was EdenFS-related, check daemon logs for errors around that time

### Determine cause

- If Scuba shows an abnormally high invocation count for a command, ask the user what is triggering those invocations (cron, watch loop, IDE plugin, etc.)
- If strace shows a clear transient issue (broken pipe, hung connection), suggest killing the process
- If strace or logs show the process is stuck in a way that indicates a bug, escalate — see [escalation.md](escalation.md)
