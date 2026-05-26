# Monitoring Systemd-Managed EdenFS

## Table of Contents
- [Scuba Dashboards](#scuba-dashboards)
- [Metrics Stages](#metrics-stages)
- [Log Locations](#log-locations)
- [Checking Service Status on Remote Hosts](#checking-service-status-on-remote-hosts)
- [Rollout Monitoring](#rollout-monitoring)

## Scuba Dashboards

All metrics are in the `edenfs_events` Scuba table.

| What | Scuba Link | Filters |
|------|-----------|---------|
| systemctl action events (start/reload) | https://fburl.com/scuba/edenfs_events/gqaiooyb | `type=systemctl_action` |
| systemctl command failures | https://fburl.com/scuba/edenfs_events/4tf01a9c | `type=systemctl_action, success=0` |
| Spot-check a specific user | https://fburl.com/scuba/edenfs_events/3kn80ys0 | Add `user=<username>` filter |
| EdenFS auto-restarted by systemd | https://fburl.com/scuba/edenfs_events/0lncj8hx | Filters on arg file age to distinguish auto-restart from intentional restart |
| General systemd start failures | https://fburl.com/scuba/edenfs_events/ecxn4ooi | `type=systemctl_action, action=start, success=0` |
| Rollout error categorization | https://fburl.com/scuba/edenfs_events/klgadxah | Groups failures by error type |
| edenfs_restarter success rate (Linux) | https://fburl.com/scuba/edenfs_events/ns0u5asy | `type=edenfs_upgrade, os=Linux` |
| edenfs_restarter success (team devservers) | https://fburl.com/scuba/edenfs_events/zx1pg5sp | Filtered to team members |
| systemctl actions (team devservers) | https://fburl.com/scuba/edenfs_events/7lngc6ts | Team-filtered |
| eden start success rate | https://fburl.com/scuba/edenfs_events/szl7180y | Overall monitoring |
| CLI usage (eden start/restart) | Check `edenfs_cli_usage` Scuba table | Track manual user interventions |

## Metrics Stages

Monitoring is organized in three stages corresponding to the restart pipeline:

### Stage 1: edenfs_upgrade Service (type = "edenfs_upgrade")

The hourly `edenfs_restarter` run. Three outcomes:

| Outcome | How to detect |
|---------|--------------|
| Ran, needed upgrade, succeeded | `type=edenfs_upgrade`, `mode` in {normal, graceful}, no failure marker |
| Ran, needed upgrade, failed | `type=edenfs_upgrade`, `mode` in {normal, graceful}, failure reason present |
| Ran, no upgrade needed, exited | No `edenfs_upgrade` event fires. The `edenfs_restarter` event fires with `num_restarts=0` |

Skip reasons (when no upgrade needed): build is recent enough, or already running the latest version.

### Stage 2: systemctl Action (type = "systemctl_action")

The actual `systemctl --user start` or `systemctl --user reload` command.

| CLI Command | systemctl Action |
|------------|-----------------|
| `eden start` / `eden restart --force` | `start` |
| `eden restart --graceful` | `reload` |

### Stage 3: Automatic Systemd Restarts

When systemd auto-restarts edenfs after an unexpected exit (crash, OOM-kill). Distinguished from intentional restarts by the **arg file age** — auto-restarts reuse the old args file, while intentional restarts write a fresh one.

Scuba: https://fburl.com/scuba/edenfs_events/0lncj8hx

## Log Locations

| Log | Path | What it contains |
|-----|------|-----------------|
| EdenFS startup log | `<state_dir>/.edenfs_startup.log` | stdout/stderr from the edenfs daemon during startup. Shows config warnings, version, initialization errors |
| EdenFS debug log | `eden debug log` (command) | Full daemon log including mount/unmount, crashes, stack traces |
| edenfs_upgrade log | `/var/facebook/logs/edenfs_upgrade.log` | edenfs_restarter output including version checks, upgrade decisions, restart attempts |
| edenfs_upgrade archived logs | `/var/facebook/logs/archive/edenfs_upgrade.log-YYYYMMDD.gz` | Rotated logs for older dates |
| System messages | `/var/log/messages` | systemd service events for edenfs@ (start, stop, fail, restart scheduling) |

### Useful grep patterns for /var/log/messages

```bash
# All edenfs service events
grep 'edenfs@' /var/log/messages | tail -30

# Service killed (OOM, signal)
grep 'edenfs@.*exited, code=killed' /var/log/messages

# Service failed with exit code
grep 'edenfs@.*Failed with result' /var/log/messages

# Restart scheduling
grep 'edenfs@.*Scheduled restart' /var/log/messages

# Start limit exceeded
grep 'edenfs@.*start-limit-hit' /var/log/messages

# edenfs_upgrade timer activity
grep 'edenfs_upgrade' /var/log/messages
```

### Useful grep patterns for eden debug log

```bash
# Find daemon start events
eden debug log | grep -i "starting edenfs" | tail -10

# Find crash stack traces
eden debug log | grep -B5 "SIGABRT\|SIGSEGV\|signal"

# Find startup completion
eden debug log | grep "Started EdenFS"

# Find takeover errors
eden debug log | grep -i "takeover\|UnixSocket"
```

## Checking Service Status on Remote Hosts

### Via sush (as root)

You cannot use `su` to switch users and run `eden status --debug` — it won't have the correct D-Bus session. Instead:

```bash
# Correct way (uses machinectl to enter the user's systemd session)
machinectl shell <username>@.host /usr/local/bin/eden status --debug

# Check eden version
machinectl shell <username>@.host /usr/local/bin/eden version

# Check eden config
machinectl shell <username>@.host /usr/local/bin/eden config
```

### XDG_RUNTIME_DIR workaround

If `machinectl` is not available, set `XDG_RUNTIME_DIR` manually:
```bash
XDG_RUNTIME_DIR=/run/user/$(id -u <username>) eden status --debug
```

### Checking prerequisites

```bash
# Is D-Bus alive for this user?
python3 -c "import socket; s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.settimeout(1); s.connect('/run/user/<UID>/bus'); print('alive'); s.close()"

# Is linger enabled? (needed for user services to persist without login session)
loginctl show-user <USERNAME> --property=Linger
```

## Rollout Monitoring

When rolling out systemd-managed EdenFS to new hosts:

1. Check overall success rate: https://fburl.com/scuba/edenfs_events/szl7180y
2. Check error categorization: https://fburl.com/scuba/edenfs_events/klgadxah
3. For failures with empty error fields, check the startup log on the host (`<state_dir>/.edenfs_startup.log`) — some errors are logged there but not captured in Scuba
4. Check if failures are from edenfs_upgrade vs manual CLI: cross-reference `edenfs_events` with `edenfs_cli_usage` table
