# Systemd-Managed EdenFS Architecture

## Table of Contents
- [Overview](#overview)
- [Service Unit File](#service-unit-file)
- [Lifecycle Operations](#lifecycle-operations)
- [Key Components](#key-components)
- [cgroup Hierarchy](#cgroup-hierarchy)
- [Memory Protection](#memory-protection)

## Overview

Systemd manages the EdenFS daemon in the **user scope** (`systemctl --user`). Each user on a host gets their own independent edenfs service instance. The service runs under the user's slice in the cgroup hierarchy:

```
/user.slice/user-<UID>.slice/user@<UID>.service/edenfs.slice/edenfs@<state-dir>.service
```

The feature is gated by config: `[experimental] systemd-managed-lifecycle = true`.

When enabled, `eden start` writes an args file to the state directory, then delegates to `systemctl --user start edenfs@<instance>`. Systemd reads the args file via `edenfsctl systemd-start` and spawns the daemon.

## Service Unit File

Location: `/usr/lib/systemd/user/edenfs@.service`

```ini
[Unit]
Description=EdenFS (state directory: %i)
AssertPathExists=/dev/fuse
StartLimitIntervalSec=120
StartLimitBurst=5

[Service]
Type=notify
NotifyAccess=all
ExecStart=/usr/local/bin/edenfsctl systemd-start --args-file /%I/.edenfs_start_args
ExecReload=/usr/local/bin/edenfsctl systemd-start --args-file /%I/.edenfs_start_args
Restart=on-failure
RestartSec=5s
Slice=edenfs.slice
KillMode=process
TimeoutStartSec=600
TimeoutStopSec=300
PassEnvironment=HOME USER LOGNAME SSH_AUTH_SOCK SSH_AGENT_PID KRB5CCNAME XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS
StandardOutput=file:/%I/.edenfs_startup.log
StandardError=file:/%I/.edenfs_startup.log

[Install]
WantedBy=default.target
```

### Key Directives Explained

| Directive | Value | Why |
|-----------|-------|-----|
| `Type=notify` | The daemon calls `sd_notify(READY=1)` after initialization completes. Systemd waits for this before marking the service active. |
| `NotifyAccess=all` | Allows any process in the service's cgroup to send notifications. Needed for graceful restart where a new daemon process sends READY=1. |
| `Restart=on-failure` | Auto-restart when the daemon exits with non-zero, is killed by a signal, or times out. Does NOT restart on clean `systemctl stop`. |
| `RestartSec=5s` | Wait 5 seconds between restart attempts. |
| `StartLimitBurst=5` | Max 5 start attempts within `StartLimitIntervalSec=120` seconds. After that, the service enters a failed state and stops retrying. |
| `KillMode=process` | Only kill the main edenfs process, not child processes (privhelper, scribe_cat). |
| `TimeoutStartSec=600` | 10 minutes for startup (EdenFS can take a long time with many checkouts). |
| `TimeoutStopSec=300` | 5 minutes for shutdown before SIGKILL. |
| `Slice=edenfs.slice` | Isolates EdenFS in its own cgroup slice for memory control. |

### The Args File

The args file (`<state_dir>/.edenfs_start_args`) is a JSON file written by `eden start` / `eden restart`. It contains the command list and environment dict needed to start the daemon. `edenfsctl systemd-start` reads this file and executes the daemon binary with those arguments.

This decouples the CLI from systemd: the CLI writes what to run, systemd controls when to run it.

## Lifecycle Operations

### eden start (systemd-managed)

1. CLI writes `.edenfs_start_args` to the state directory
2. CLI runs `systemctl --user start edenfs@<instance>`
3. Systemd executes `ExecStart`: `edenfsctl systemd-start --args-file <path>`
4. `systemd-start` reads the args file, starts the edenfs daemon with `--foreground`
5. Daemon initializes, calls `sd_notify(READY=1)`
6. Systemd marks service as active

The daemon runs with `--foreground` (no fork). The old parent-child fork pattern is unnecessary because systemd already handles daemonization, terminal detachment, and process lifecycle tracking.

### eden stop (systemd-managed)

1. CLI stops auxiliary processes (redirections, Myles)
2. CLI runs `systemctl --user stop edenfs@<instance>`
3. Systemd sends SIGTERM to the main edenfs process
4. EdenFS performs cleanup: cancel requests, stop Thrift server, unmount all, close storage, shutdown privhelper
5. If not exited within `TimeoutStopSec=300`, systemd sends SIGKILL
6. Service enters "inactive" state — `Restart=on-failure` does NOT trigger because this is a clean stop

### eden restart (systemd-managed)

1. CLI stops auxiliary processes
2. CLI runs `systemctl --user restart edenfs@<instance>`
3. Systemd executes stop (SIGTERM) then start (ExecStart)
4. Same as eden start from step 3 onward

### eden restart --graceful (systemd-managed)

1. CLI writes updated `.edenfs_start_args`
2. CLI runs `systemctl --user reload edenfs@<instance>`
3. Systemd executes `ExecReload`: `edenfsctl systemd-start --args-file <path>` (with `--takeover`)
4. New daemon connects to old daemon via Unix socket for takeover
5. Old daemon transfers mount points to new daemon
6. New daemon calls `sd_notify(MAINPID=<new_pid>)` — systemd now tracks the new process
7. Old daemon exits — systemd does NOT restart it because it received the MAINPID notification

### Auto-restart (crash/OOM recovery)

When the daemon exits unexpectedly (non-zero exit, SIGKILL from OOM, SIGABRT from crash):
1. Systemd detects the failure (`Restart=on-failure`)
2. Waits `RestartSec=5s`
3. Executes ExecStart again
4. If startup fails, retries up to `StartLimitBurst=5` times within `StartLimitIntervalSec=120s`
5. After exceeding the burst limit, the service enters failed state

The auto-restart is distinguished from intentional restarts by the **arg file age** — an auto-restart reuses the existing args file (old timestamp), while an intentional restart writes a fresh args file.

## Key Components

### edenfs_upgrade Timer (System Scope)

The `edenfs_upgrade.timer` runs hourly (system-wide, managed by Chef) and triggers `edenfs_upgrade.service`, which runs `edenfs_restarter`.

```
edenfs_upgrade.timer → edenfs_upgrade.service → systemd-run --scope edenfs_restarter
```

`edenfs_restarter` checks if the running EdenFS is outdated and triggers a graceful restart if needed. Under systemd, this translates to `systemctl --user reload edenfs@<instance>`.

**Timer config:** `/etc/systemd/timers/edenfs_upgrade.timer`
**Service config:** `/etc/systemd/timers/edenfs_upgrade.service`
**Logs:** `/var/facebook/logs/edenfs_upgrade.log`

### edenfsctl systemd-start

A subcommand that reads the args file and starts the edenfs daemon. This is what systemd's `ExecStart` and `ExecReload` call. It replaces the old call chain where `eden start` would directly spawn the daemon.

### sd_notify Integration

EdenFS calls `sd_notify()` at key points:
- `READY=1` — after successful initialization (replaces the old pipe-based parent-child signaling)
- `MAINPID=<pid>` — during graceful restart, to tell systemd to track the new daemon process

## cgroup Hierarchy

```
user.slice/
  user-<UID>.slice/
    user@<UID>.service/
      edenfs.slice/                    ← EdenFS's dedicated slice
        edenfs@<state-dir>.service/    ← The actual service
          ├── edenfs (main process)
          ├── edenfs_privhelper
          └── scribe_cat (multiple)
```

The `edenfs.slice` provides cgroup isolation. Memory knobs (`MemoryLow`, `MemoryHigh`) can be set on this slice to protect EdenFS from OOM or throttle it to prevent dominating memory.

## Memory Protection

Two cgroup memory controls work together to keep EdenFS alive:

| Control | Effect |
|---------|--------|
| `MemoryLow` | Protection floor — kernel avoids reclaiming from EdenFS below this threshold, prefers to reclaim from siblings (e.g., buck2) |
| `MemoryHigh` | Throttle ceiling — when exceeded, kernel forces aggressive reclaim and throttles allocating threads. EdenFS gets slow but stays alive. NOT a kill threshold. |

Only `MemoryMax` (if set) triggers the kernel OOM killer. `MemoryHigh` creates backpressure so EdenFS self-reclaims before reaching dangerous sizes, making it less attractive to fb-oomd.

The tradeoff: `MemoryHigh` throttling degrades EdenFS performance (slow file operations, checkouts, status). Better than being killed, but still painful for developer workflows. Tune carefully.
