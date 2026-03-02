# Blackbox Guide

**How to read `sl blackbox` output, interpret metrics, and search rotated logs**

## Finding slow commands

```bash
sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'
```
- `--start 1440` = look back 24 hours (default is only 15 minutes)
- Output shows: timestamp, PID, exit code, duration in ms, max RSS in bytes
- Pick the PID to investigate, then get all its events:

```bash
sl blackbox --start 1440 | grep "<PID>"
```

## Line types and what they tell you

**[command]** — what command ran
```
[command] ["hg.real", "ssl", "--commit-info"] started by uid 160048 as pid 2104002 with nice 0
```
- `hg.real` is the internal binary name; the user-facing command is `sl`

**[process_tree]** — who launched it
```
[process_tree] ... /usr/libexec/devenv-backup (2100773) -> /usr/local/bin/sl (2103996) -> (this process)
```
- Distinguishes user-initiated commands from automated ones (devenv-backup, cron, ISL)
- Important for triage: automated background commands may not need user-facing fixes

**[legacy][visibility]** — visible draft heads
```
[legacy][visibility] read 225 heads: 7ab846f9cf63, ...
```
- 200-300 heads is normal; this alone is not evidence of a problem
- 1000+ heads may indicate excessive drafts

**[legacy][metrics]** — network and fetch metrics (most important for ruling out causes)

*EdenAPI (network):*
```json
{"http":{"edenapi":{"num":{"requests":21},"total":{"request":{"time":{"ms":75}},"rx":{"bytes":26905},"tx":{"bytes":697}}}}}
```
- `request.time.ms` = total network time. If small relative to total duration, network is NOT the bottleneck.

*scmstore (file/tree fetches):*
```json
{"scmstore":{"file":{"api":{"hg":{"refresh":{"calls":1}}},"flush":1},"tree":{"flush":1}}}
```
- No `"fetch"` sub-object with cache hits/misses = no significant file/tree fetches occurred
- A fetch-heavy command shows: `"fetch":{"indexedlog":{"cache":{"hits":3,"keys":4,...}}}`

**[command_finish]** — final summary
```
[command_finish] exited 0 in 18763 ms, max RSS: 2921730048 bytes
```
- Exit code (`0` = success, `255` = error), duration in ms, max RSS in bytes (divide by 1024^3 for GB)

**[tracing]** — binary tracing data (not human-readable)
```
[tracing] (binary data of 15917 bytes)
```
- Contains detailed per-function call tree with microsecond timing, but no CLI command exists to decode it retroactively
- To get readable tracing, must be set up BEFORE the command: `EDENSCM_TRACE_OUTPUT=/tmp/trace.txt sl <command>`

## Finding time gaps

Look at timestamps between consecutive events. A large gap with no events means the work happened during that gap, but blackbox does not record intermediate steps. The breakdown is in the binary tracing blob but cannot currently be decoded from past blackbox entries.

## Quick reference — blackbox commands

```bash
sl blackbox                              # last 15 minutes (default)
sl blackbox --start 60                   # last hour
sl blackbox --start 1440                 # last 24 hours
sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 5000, 999999]}}'   # slow commands
sl blackbox --start 60 --pattern '{"network": "_"}'    # network events
sl blackbox --start 60 --pattern '{"blocked": "_"}'    # blocked events
sl blackbox --start 60 --pattern '{"watchman": "_"}'   # watchman events
sl blackbox --start 60 --debug                         # raw JSON
sl blackbox --start 1440 | grep "<PID>"                # all events for a PID
```

## Other useful diagnostic commands

```bash
# EdenFS health and state
eden list --json             # all mounts: state (RUNNING/NOT_RUNNING), data_dir, backing_repo
eden status                  # EdenFS health + pid
eden uptime                  # how long EdenFS has been running (detect recent crashes)
eden pid                     # EdenFS daemon PID
eden doctor                  # 25+ checks with auto-repair
eden debug log               # open/stream the EdenFS log
eden stats                   # memory and inode statistics
eden fsck                    # scan overlay for corruption
eden gc                      # free caches to reduce disk & memory

# EdenFS redirections
eden redirect list           # list all redirections and their state
eden redirect fixup          # fix misconfigured redirections
eden redirect del <redir>    # delete a specific broken redirection

# EdenFS tracing (some have --retroactive, some must be started before the command)
eden trace sl --retroactive      # past object fetches from buffer (has retroactive)
eden trace inode --retroactive   # past inode loads/materializations (has retroactive)
eden trace thrift --retroactive  # past thrift requests (has retroactive)
eden trace fs                    # live filesystem ops (NO retroactive — start first, then run command)
eden trace task                  # live internal tasks (NO retroactive — start first, then run command)

# Sapling diagnostics
sl doctor                    # repairs treestate, metalog, segments, mutation store
sl debugnetwork              # network latency + bandwidth test to Mononoke
sl debugnetworkdoctor        # pass/fail network health check
sl cloud status              # commit cloud workspace state
sl debugrunlog               # currently running sl commands (for hang diagnosis)
sl debugrunlog --ended       # recently finished commands (kept ~1 hour)
sl debugbacktrace <PID>      # extract backtrace from a stuck sl process
sl debugprocesstree          # process tree for all sl-related processes
sl debugmutation -r 'draft() & date(-4)' -t 'since 4d ago'  # mutation history for recent drafts
sl debugmetalog -t 'since 2d ago'  # metalog changes over last 2 days
sl debugchangelog            # changelog backend info (segments, zstore, etc.)
sl debugvisibleheads         # list all visible draft heads with commit messages
sl debugrebuildchangelog     # rebuild changelog from server (destructive, last resort)
sl debugundohistory -l       # list recent undo-able operations
sl debugconflictcontext      # context about conflicting files during merge
sl debugpickmergetool <file> # which merge tool will be used for a file
sl debugcheckstate           # validate dirstate correctness
```

## Rotated blackbox logs

`sl blackbox --start N` only searches the current blackbox log file. Blackbox logs rotate when they reach ~5MB. Rotated logs are kept at:

```
.hg/blackbox.log       # current (active)
.hg/blackbox.log.1     # most recent rotated
.hg/blackbox.log.2     # older
.hg/blackbox.log.3     # older still
... up to .hg/blackbox.log.6 or more
```

**When to check rotated logs:**
- The event you're investigating is older than what `sl blackbox --start 10080` (7 days) returns
- `sl blackbox` returns no results for a PID or commit hash you know exists
- The user reports an issue from days or weeks ago

**How to search rotated logs:**
```bash
# List available rotated logs with dates
ls -la .hg/blackbox.log*

# Search for a specific commit hash
grep "82c0f45f487e" .hg/blackbox.log.1

# Search for a specific command
grep "amend" .hg/blackbox.log.1 | grep "\[command\]"

# Search for a specific PID across all rotated logs
grep "3958309" .hg/blackbox.log .hg/blackbox.log.1 .hg/blackbox.log.2

# Find all events for a PID
grep "3958309" .hg/blackbox.log.1
```

**Important differences from `sl blackbox` output:**
- The `--pattern` JSON filter syntax does NOT work on rotated log files — use grep instead
- Rotated logs may use an **older logging format**: `[visibility]>` instead of `[legacy][visibility]`, `[command]>` instead of `[command]`, etc. The content is the same but the prefix format differs.
- A `[command]` entry without a matching `[command_finish]` may indicate the process was killed or crashed, OR it may be a format difference in older logs.

**Always check rotated logs early** if the user reports an issue from more than ~2 days ago.
