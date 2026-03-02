# EdenFS Diagnosis

**EdenFS problems — crashes, restarts, redirections, health, tracing, logs, disk space**

## Run these first for any EdenFS issue

```bash
eden status                  # is EdenFS running and healthy? shows pid
eden uptime                  # how long has EdenFS been running (detect recent crashes/restarts)
eden list --json             # all mounts with state (RUNNING/NOT_RUNNING) and backing repo paths
eden doctor                  # 25+ health checks with auto-repair
eden debug log               # open/stream the EdenFS log (simpler than finding the log file manually)
```

## EdenFS crashes / restart stuck

```bash
# Check uptime — short uptime indicates recent crash/restart
eden uptime

# Read the EdenFS log for crash evidence
eden debug log
# Look for: fatal errors, stack traces, OOM kills, assertion failures

# Check if OS OOM-killed EdenFS
dmesg | grep -i -E 'oom|eden|killed' | tail -20
# With timestamps:
dmesg -T | grep -i -E 'oom|eden|killed' | tail -20

# Check systemd journal for edenfs events (Linux)
journalctl -u edenfs --since "1 hour ago" --no-pager | tail -30

# Check EdenFS process state
eden pid                     # get the EdenFS daemon PID
eden stats                   # memory/inode statistics

# If EdenFS won't start or restart is stuck:
eden status                  # check current state
eden stop                    # force stop first
eden start                   # then start fresh
# If still stuck, check the log: eden debug log
```

## Diagnosing OOM kills

When EdenFS crashes and `dmesg` shows an OOM kill:

```bash
# 1. Confirm OOM kill and find timestamp
dmesg -T | grep -i oom | tail -10

# 2. Get full OOM report (shows memory state at kill time)
dmesg -T | grep -A 30 'Out of memory'

# 3. Find the eden log path
EDEN_LOG="$(dirname "$(eden list --json | python3 -c "import sys,json; print(next(iter(json.load(sys.stdin).values()))['data_dir'])")")/logs/edenfs.log"

# 4. Correlate: what was EdenFS doing just before the kill?
# Use the timestamp from dmesg to search the eden log
grep "<HH:MM>" "$EDEN_LOG" | tail -20
# Look for: Heavy fetches, GC activity, high inode counts

# 5. Check what process was driving memory consumption
# Query Scuba edenfs_events for fetch_heavy/big_walk around the crash time
# Look at client_cmdline to identify the culprit (often buck2d, IDE indexers)

# 6. Check system memory state
free -h
cat /proc/meminfo | head -10
```

**Common OOM causes:**
- **buck2 build** driving massive fetches through EdenFS → millions of inodes loaded
- **IDE indexing** (VS Code, IntelliJ) walking the entire tree
- **Multiple large repos** all active simultaneously
- **Memory leak in EdenFS** (check if uptime before crash was very long)

## Timestamp correlation

When investigating a crash or error, correlate across multiple log sources:

```bash
# 1. Find the event timestamp from one source
# e.g., eden log shows a crash at 14:32:15

# 2. Check system log at that time
dmesg -T | grep "14:32"
journalctl --since "14:30" --until "14:35" --no-pager

# 3. Check blackbox at that time (if a repo exists)
sl blackbox --start 60 | grep "14:32"

# 4. Check eden log around that time
grep "14:3[0-5]" "$EDEN_LOG" | head -30

# 5. Build a timeline: what happened in what order?
# System event → EdenFS reaction → Sapling impact
```

## EdenFS redirection issues

```bash
# List all current redirections and their state
eden redirect list

# Fix misconfigured redirections (restores expected state)
eden redirect fixup

# Delete a specific broken redirection
eden redirect del <redirection>

# Add a new redirection (e.g., for scratch/temp directories)
eden redirect add path/to/dir bind
```

- Redirections are bind mounts that redirect specific directories (like `buck-out`) to a different location
- `eden redirect fixup` is the first thing to try for redirection issues — it restores the expected configuration
- Orphaned redirections (for mounts that no longer exist) can be seen in `eden du` output and cleaned up with `eden redirect del`

## EdenFS monitoring and resource usage

```bash
# Memory and inode statistics
eden stats                   # loaded inode counts, memory usage

# Free caches to reduce disk & memory usage
eden gc

# Check overlay/filesystem integrity
eden fsck                    # scan for overlay corruption
eden fsck --check-only       # scan without attempting repair
eden fsck --verbose          # detailed output about issues found
```

Note: `eden top` and `eden minitop` are interactive/TUI commands — do not run from this skill. Use `eden stats` for programmatic resource data.

## EdenFS tracing — for diagnosing slow or problematic EdenFS operations

Some trace commands have `--retroactive` (can show past events from a buffer), others must be started BEFORE the problematic command runs.

```bash
# Trace Sapling object fetches (has --retroactive)
eden trace sl                    # live: watch blob/tree fetches as they happen
eden trace sl --retroactive      # past: show last N fetches from buffer

# Trace inode state changes — loads and materializations (has --retroactive)
eden trace inode                 # live: watch inode loads/materializations
eden trace inode --retroactive   # past: show last N inode changes

# Trace thrift requests to EdenFS (has --retroactive)
eden trace thrift                # live: watch thrift calls with timing
eden trace thrift --retroactive  # past: show last N thrift requests

# Trace filesystem operations — NO retroactive, must start BEFORE the command
eden trace fs                    # live: watch all FUSE/NFS operations
eden trace fs --reads            # limit to read operations
eden trace fs --writes           # limit to write operations

# Trace internal EdenFS tasks — NO retroactive, must start BEFORE the command
eden trace task                  # live: watch instrumented internal tasks
eden trace task --chrome-trace   # output in Chrome trace format for analysis
```

**Using trace for diagnosis:**
- For **slow file operations**: start `eden trace fs` in one terminal, run the slow command in another, collect the trace output
- For **excessive fetches from server**: use `eden trace sl --retroactive` to see recent fetch activity, or `eden trace sl` live to watch fetches during a command
- For **high inode materialization** (memory pressure): use `eden trace inode --retroactive` to see what's being loaded
- For **thrift performance issues**: use `eden trace thrift --retroactive` to see slow thrift calls

The retroactive buffer defaults to 100 events. Configure with:
```ini
[telemetry]
activitybuffer-max-events = 100
```

## Certificate issues

```bash
# EdenFS doctor checks certs at /var/facebook/credentials/$USER/x509/$USER.pem
eden doctor

# Fix cert issues:
# Linux: kdestroy ; kinit ; sks renew (or /usr/local/bin/sks renew)
# macOS: kdestroy ; kinit ; /opt/facebook/bin/sks renew
# General: fixmyserver (Linux) / fixmymac (macOS) for underlying cert/connection issues
```

## EdenFS log analysis

**Finding the eden log:**
1. Run `eden list --json` — the `data_dir` field for any mount gives you the `.eden/clients/<name>` path
2. The `.eden` folder is the parent of `clients/` — e.g., if `data_dir` is `/data/users/foo/.eden/clients/fbsource`, then the eden root is `/data/users/foo/.eden/`
3. The log is at `<eden_root>/logs/edenfs.log`
4. Rotated logs are in the same directory as `.gz` files (e.g., `edenfs.log-20251109.gz`)

The log can be 100K+ lines. Do not read the whole file — grep for specific patterns.

**Grep patterns for diagnosing slow EdenFS operations:**

```bash
# First, find the eden log path
EDEN_LOG="$(dirname "$(eden list --json | python3 -c "import sys,json; print(next(iter(json.load(sys.stdin).values()))['data_dir'])")")/logs/edenfs.log"

# Slow periodic tasks (GC, store maintenance, recovery)
grep "slow periodic task" "$EDEN_LOG" | tail -20

# Heavy fetch activity (which process is hammering EdenFS and how many fetches)
grep "Heavy fetches" "$EDEN_LOG" | tail -20

# GC activity per mount (timing and inode counts before/after)
grep "GC for:" "$EDEN_LOG" | tail -20

# Errors and warnings
grep "^E\|^W" "$EDEN_LOG" | tail -30

# Slow operations
grep -i "slow\|timeout\|took.*seconds" "$EDEN_LOG" | tail -20
```

**What to look for:**
- **`slow periodic task: local_store took Nms`** — the local object store is slow, may indicate disk I/O issues
- **`Heavy fetches (N) from process <name>`** — a process (often `buck2d`) is doing massive fetches through EdenFS. The number increments in batches of 100,000. Look at which process and how fast the count climbs to understand the load.
- **`GC for: <mount>, completed in N seconds, total number of inodes after GC: N`** — if GC is slow or inode count is very high after GC, the mount may have too many loaded inodes
- **`setpriority failed`** — thread priority warnings, usually harmless
- **`streaming client disconnected`** — thrift client disconnects, may correlate with operation failures
- **Error lines (starting with `E`)** — actual errors, always worth reading

## Correlating EdenFS log with Sapling commands

When diagnosing a slow or hanging Sapling command, you can correlate it with EdenFS activity by timestamp and by the `resetParentCommits` thrift call.

**`resetParentCommits`** is the EdenFS thrift call that Sapling makes to tell EdenFS "the working copy is now at this commit." Commands like `sl amend`, `sl commit`, `sl rebase`, `sl goto`, and `sl checkout` all call this.

```bash
EDEN_LOG="$(dirname "$(eden list --json | python3 -c "import sys,json; print(next(iter(json.load(sys.stdin).values()))['data_dir'])")")/logs/edenfs.log"

# Find resetParentCommits calls around a specific time (replace MMDD HH with target date/hour)
grep "resetParentCommits\|resetting snapshot" "$EDEN_LOG" | grep "<MMDD> <HH>:"

# Find calls involving a specific commit hash
grep "<commit_hash>" "$EDEN_LOG"
```

**Interpreting the timing:**
- `resetParentCommits() took N us` — how long EdenFS took to process the checkout. If this is small (< 100ms), EdenFS is NOT the bottleneck.
- **Gap between `[command]` in blackbox and `resetParentCommits` in EdenFS log** — time spent by Sapling before calling EdenFS (status, hooks, conflict checks, etc.). This is where hangs often occur.

**Common verbose log noise to ignore:**
- `SaplingBackingStore.cpp: commit <hash> has manifest node <hash>` — normal commit-to-manifest resolution logs. High volume does NOT indicate a problem.
- `setpriority failed` — thread priority warnings, harmless
- `TomlFileConfigSource.cpp: Ignoring unknown key` — config key not recognized, harmless

## Disk space diagnosis

**Run these commands:**
```bash
# EdenFS-specific disk breakdown (all platforms)
eden du --fast

# General disk usage (Linux / ODS / devservers)
df -h
```

**On macOS, also run:**
```bash
diskutil apfs list
```

**Interpreting `eden du --fast` output:**
- **Materialized files** — files that have been accessed and written to the local overlay. High values mean many files have been loaded from the backing store.
- **Redirections** — bind mounts redirecting specific directories (e.g., `buck-out`) to a different location. These are expected and normal.
- **Orphaned redirections** — redirections for mounts that no longer exist. Can be cleaned up.
- **Ignored files** — files in `.gitignore`/`.hgignore` patterns (build artifacts, logs, etc.)
- **Backing repos** — local cache of tree and file data fetched from Mononoke. Can grow large. Shared across mounts.
- **Shared space** — space shared across all EdenFS mounts (logs, backing stores, etc.)

**What to look for:**
- **`df -h` shows filesystem nearly full** — may cause EdenFS failures, checkout errors, or build failures. Free space or expand storage.
- **Large backing repos** — if backing repo cache is very large, consider `eden gc` or clearing the backing store cache.
- **Large materialized files** — indicates many files loaded into the overlay; may be caused by a build system or IDE indexing.
- **Large orphaned redirections** — safe to clean up, recovers disk space.

## Common EdenFS problems

- **EdenFS not running** — `eden status` shows "not healthy". Fix: `eden start` or `eden restart`
- **Stale mount** — I/O errors, files appear missing, `Stale NFS file handle` (Errno 70 on macOS) or `Transport endpoint is not connected` (Linux FUSE). Fix escalation ladder:
  1. `eden doctor` — auto-fixes most stale mounts by remounting
  2. `eden unmount <path> && eden mount <path>` — manual remount
  3. If steps 1-2 fail (e.g., stale NFS handle blocks stat/mkdir on the mountpoint):
     - Check if a stale kernel mount exists: `mount | grep <repo_name>`
     - macOS (NFS): `sudo umount -f <path>`, then `eden mount <path>`
     - Linux (FUSE): `sudo umount -l <path>` (lazy unmount), then `eden mount <path>`
  4. If all else fails: `eden rm <path>` + reclone (`fbclone <repo> --eden`). **First** check for uncommitted work — OD backup at https://www.internalfb.com/intern/ondemand/backup/
- **Restart stuck** — check `eden debug log` for what's blocking. May need `eden stop` first, then `eden start`
- **Redirection broken** — `eden redirect list` to see state, `eden redirect fixup` to repair
- **High memory / OOM** — `eden stats` for inode count, `eden gc` to free caches
- **Overlay corruption** — `eden fsck` to scan and repair, `eden doctor` may also catch this
- **Certificate expired** — network operations fail, `eden doctor` reports cert issues. Fix: `kdestroy ; kinit ; sks renew`
- **Need to reclone** — if EdenFS is beyond repair: save uncommitted changes (check OD backup at https://www.internalfb.com/intern/ondemand/backup/), then `eden rm <repo>` followed by `fbclone <repo> --eden`

## When eden doctor's fix fails

`eden doctor` auto-repairs many issues, but sometimes its fix attempt fails. When this happens, read the error output from doctor carefully — it tells you why the fix failed.

**Common doctor fix failures:**

- **Remount fails with `Stale NFS file handle` (Errno 70)** — the old NFS mount is still registered in the kernel but EdenFS is no longer serving it. Any filesystem operation on the mountpoint (stat, mkdir, is_dir) returns Errno 70. Doctor can't remount because it can't create/verify the mountpoint directory. Fix: force-unmount the stale kernel mount first (see "Stale mount" escalation ladder above), then `eden mount <path>`.

- **Remount fails with `FileExistsError`** — the mountpoint path exists (possibly as a stale mount or regular directory). If it's a stale mount, force-unmount first. If it's a regular directory that shouldn't be there, check what's in it before removing.

- **Remount fails with disk space errors** — the filesystem is full. Free space first (`eden gc`, remove build artifacts, check `df -h`), then retry the mount.

- **`hg doctor` fails during remount** — eden doctor sometimes runs `hg doctor` on the backing repo before remounting. If this step fails, check the backing repo state at the path shown in `eden list --json` under `backing_repo`.

**General approach when doctor fails:**
1. Read the full error output — trace the chain of what doctor tried and where it failed
2. Fix the underlying blocker (stale mount, disk space, permissions)
3. Retry `eden mount <path>` directly (don't re-run all of doctor)
4. Verify with `eden status` and `eden list --json`

## Platform-specific mount behavior

EdenFS uses different filesystem protocols depending on the platform. This affects how mounts go stale and how to recover them.

**macOS — NFS**
- EdenFS serves files via an NFS server
- Mountpoints are under `/Users/<user>/`
- Stale mount error: `Stale NFS file handle` (Errno 70)
- Force unmount: `sudo umount -f <path>` or `sudo diskutil unmount force <path>`
- Spotlight/mds indexing can cause excessive file access — check `fs_usage` or `eden trace sl` if EdenFS is under unexplained load

**Linux — FUSE (or NFS on some configurations)**
- EdenFS typically uses FUSE on Linux
- Mountpoints are under `/data/users/<user>/`
- Stale mount error: `Transport endpoint is not connected`
- Force unmount: `sudo umount -l <path>` (lazy unmount) or `fusermount -u <path>`
- Check `/proc/mounts` or `mount` to see if the stale mount is still registered

**Windows — PrjFS (Projected File System)**
- EdenFS uses Windows Projected File System (PrjFS) — working copy persists between EdenFS restarts
- Mountpoints are typically under `C:\open\` or user-chosen paths
- Clone command: `fbclone <repo> --eden` (uses `hg clone --eden` internally)
- Backing repos stored separately (e.g., `c:\open\eden-backing-repos\<repo>`)
- No `mount`/`umount` commands — PrjFS mounts are managed differently than FUSE/NFS
- Stale mount recovery: `eden rm <path>` + reclone; no force-unmount equivalent
- Common Windows-specific issues:
  - **Partially installed host** — `fbclone` errors pointing to `https://fburl.com/windows_fbclone_failure`. Follow the wiki steps to complete EdenFS setup.
  - **PrjFS virtualization issues** — files not appearing or stale content. Try `eden doctor`, then `eden restart`
  - **WER crash dumps** — Windows Error Reporting dumps are in the path from registry key `SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\DumpFolder`
- Diagnostic commands: `eden rage` collects PrjFS-specific counters (`prjfs\.*`) on Windows
- No RPM on Windows — use `eden version` and `sl --version` for version info

## FilteredFS / eden-sparse issues

FilteredFS (eden-sparse) uses filter IDs stored in the **backing repo's** filter store (an indexedlog at `<shared_dot_hg_path>/filters/`) to track which files are included in a sparse checkout. These issues can occur on **any platform** (Linux, macOS, Windows).

**How it works:** When `sl sparse enable <profile>` runs, Sapling computes a V1 filter ID (a partial Blake3 hash of the sparse profile paths + commit ID), stores the filter data in the backing repo's indexedlog, and passes the ID to EdenFS. EdenFS embeds this filter ID into its RootIds and ObjectIds. Later, when EdenFS needs to evaluate the filter (e.g., during tree traversal), it calls back into Sapling via FFI, which looks up the filter data from the same indexedlog.

**Symptoms:**
- `Failed to get filter` / `Failed to find a stored Filter for ID: V1(V1, [...])`
- `UnexpectedMountProblem` from `eden doctor` on a running mount
- Clone fails with filter-related errors

**Root causes** (only two possibilities):
1. **Backing repo corruption** — the filter store (indexedlog) in the backing repo lost the filter entry
2. **Backing repo mismatch** — the fronting repo (mount) was recloned or reconnected using a different backing repo than the one that originally stored the filter. The filter IDs embedded in EdenFS's inode overlay reference filters that don't exist in the new backing repo.

**Diagnosis:**
```bash
# Check if FilteredFS is enabled and what filter config is active
edenfsctl fsconfig --all

# Check mount state — look at backing_repo path for each mount
eden list --json

# Check if the mount is actually functional despite the doctor error
eden status
sl status          # does the working copy work?
sl sparse show     # what sparse profile is active?

# Verify the backing repo path — is this the expected backing repo?
# Compare the backing_repo in eden list --json with what fbclone would use
```

**When this happens during clone:**
- The filter ID referenced during checkout can't be found in the backing repo's filter store
- Workaround: clone with FilteredFS disabled (`--config clone.use-eden-sparse=false` or equivalent)

**When this happens on an existing mount (eden doctor reports it):**
- The mount may still be functional for normal operations — check with `sl status`
- The error message does not distinguish between corruption and mismatch — you can't tell from the traceback alone. Try the simpler fix first:
- **Note:** FilteredFS repos use `sl filteredfs` commands, NOT `sl sparse`. `sl sparse` commands abort on edensparse repos.
- Fixes (try in order):
  1. `sl filteredfs show` — check what filter is currently enabled.
  2. `sl filteredfs switch <profile>` — switch to the same profile, which triggers a new filter computation and store. If this fixes the doctor error, the cause was corruption.
  3. If switch didn't help, check the backing repo path in `eden list --json`. If it's pointing to a different backing repo than expected (mismatch), re-applying the filter can't help because EdenFS has old filter IDs baked into its inode overlay. Fix: `eden rm <path>` + reclone (check OD backup at https://www.internalfb.com/intern/ondemand/backup/ first).
- If the mount works fine for daily operations and only doctor reports the error, it may be safe to continue working while filing a bug
