---
name: edenfs-systemd-triage
description: Triage systemd-managed EdenFS issues on Linux devservers and OnDemands. Use when investigating EdenFS service failures, unexpected restarts, systemctl errors, edenfs_upgrade/edenfs_restarter problems, or when a user reports EdenFS is down on a systemd-enabled host. Also use when someone asks how systemd-managed EdenFS works, how to monitor it, or how to check its health. Use when asked to build a timeline of EdenFS lifecycle events, show edenfs restart history, or understand how edenfs reached its current state. Trigger on mentions of edenfs systemd, edenfs@ service, edenfs_upgrade timer, edenfs auto-restart, eden status --debug, systemctl edenfs, edenfs lifecycle management, edenfs timeline, edenfs restart history, or "what happened to edenfs".
allowed-tools: [Read, Bash, Agent]
metadata:
  oncalls:
    - 'scm_client_infra'
  strict: true
---

# EdenFS Systemd Triage

This skill helps EdenFS team members understand, monitor, and triage systemd-managed EdenFS.

## Quick Orientation

Systemd manages the EdenFS daemon lifecycle on Linux (devservers and OnDemands). When EdenFS exits unexpectedly (crash, OOM-kill), systemd automatically restarts it — no user intervention needed.

**Two systemd components to know:**

| Component | What it does | Scope |
|-----------|-------------|-------|
| `edenfs@.service` | User-scoped service managing the edenfs daemon lifecycle (auto-restart on failure) | Per-user (`systemctl --user`) |
| `edenfs_upgrade.timer` | System-scoped hourly timer that runs `edenfs_restarter` to upgrade edenfs gracefully | System-wide (`systemctl`) |

**Config gate:** `[experimental] systemd-managed-lifecycle = true` in eden config.

## How to Use This Skill

Read the reference file that matches your need:

| Need | Reference File | When to Read |
|------|---------------|-------------|
| Understand the architecture | `references/architecture.md` | How systemd-managed EdenFS works, service unit file, lifecycle operations |
| Check health & monitor | `references/monitoring.md` | Scuba queries, dashboards, success rate metrics, rollout monitoring |
| Build a lifecycle timeline | `references/timeline.md` | Reconstruct chronological EdenFS events from Scuba + local logs + systemd properties to understand how the system reached its current state |
| Triage a specific failure | `references/triage-playbook.md` | Step-by-step procedures for common failure scenarios |
| Identify known failure patterns | `references/common-failures.md` | Error signatures, root causes, and fixes for known issues |

## First-Response Checklist

When triaging an EdenFS systemd issue, start here:

1. **Is the user on systemd?** Check: `eden config | grep systemd-managed`
2. **What's the service status?** Check: `eden status --debug`
3. **What does Scuba say?** Check systemctl action failures: https://fburl.com/scuba/edenfs_events/4tf01a9c
4. **What do the logs say?** Check: `/var/facebook/logs/edenfs_upgrade.log` and `/var/log/messages`

If you need to run commands on a user's machine via `sush`, you cannot use `su` — you must use:
```
machinectl shell <username>@.host /usr/local/bin/eden status --debug
```

## Diagnostic Commands (Safe to Run Locally)

These are read-only and safe to execute automatically:

```bash
# Check if systemd-managed
eden config | grep systemd-managed

# Full service status with systemd details
eden status --debug

# Check eden version mismatch
eden version

# Check systemd service properties (restart policy, crash counters, timestamps)
systemctl --user show edenfs@home-$(whoami)-local-.eden.service \
  --property=Id,Type,Restart,RestartUSec,StartLimitIntervalUSec,StartLimitBurst,NRestarts,ExecMainStartTimestamp,ExecMainPID,ExecMainCode,ExecMainStatus,ActiveState,SubState,Result,InvocationID,ActiveEnterTimestamp,ActiveExitTimestamp,InactiveEnterTimestamp,InactiveExitTimestamp

# Check eden logs for recent errors
eden debug log | tail -50

# Check startup log
cat <state_dir>/.edenfs_startup.log

# Check system messages for edenfs service events
grep 'edenfs@' /var/log/messages | tail -20

# Check edenfs_upgrade logs
tail -50 /var/facebook/logs/edenfs_upgrade.log

# Check kernel OOM kills
dmesg | grep -i -E '(edenfs|oom|killed)' | tail -20

# Check dbus connectivity (needed for systemctl --user)
python3 -c "import socket; s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.settimeout(1); s.connect('/run/user/$(id -u)/bus'); print('alive'); s.close()"

# Check linger (needed for user services to persist)
loginctl show-user $(whoami) --property=Linger
```

## Hostname for Scuba Queries

**CRITICAL**: Scuba uses the short hostname (e.g., `devvm21611.cco0`), NOT the FQDN returned by `hostname` (e.g., `devvm21611.cco0.facebook.com`). Always strip the `.facebook.com` suffix:

```bash
hostname | sed 's/\.facebook\.com$//'
```

Querying Scuba with the FQDN will silently return zero results.
