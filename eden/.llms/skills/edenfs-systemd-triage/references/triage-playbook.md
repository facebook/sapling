# Triage Playbook for Systemd-Managed EdenFS

## Table of Contents
- [Build a Timeline](#build-a-timeline)
- [Scenario: EdenFS is Down](#scenario-edenfs-is-down)
- [Scenario: Systemd Service Failed but EdenFS Running](#scenario-systemd-service-failed-but-edenfs-running)
- [Scenario: Restart Loop](#scenario-restart-loop)
- [Scenario: Graceful Restart Failing](#scenario-graceful-restart-failing)
- [Scenario: edenfs_upgrade Failures](#scenario-edenfs_upgrade-failures)
- [Testing Changes Locally](#testing-changes-locally)

## Build a Timeline

When triaging, always reconstruct events in chronological order. This is the single most important step — without a timeline, you're guessing.

1. **Eden crash** — find the crash in eden debug log (look for stack traces, `SIGABRT`, etc.)
   ```bash
   eden debug log | grep -B5 "SIGABRT\|SIGSEGV\|signal\|abort" | tail -30
   ```

2. **User manual start** — check Scuba CLI usage for `eden start` / `eden restart`
   - Scuba table: `edenfs_cli_usage`
   - Filter by user and time range

3. **Systemd service events** — check `/var/log/messages` for `edenfs@` entries
   ```bash
   grep 'edenfs@' /var/log/messages | tail -30
   ```

4. **Eden startup log** — look for `Starting edenfs <version>, pid <PID>, session_id <SID>`
   ```bash
   eden debug log | grep "Starting edenfs" | tail -10
   ```

5. **Eden ready** — look for `Started EdenFS (pid ...) in Xs`
   ```bash
   eden debug log | grep "Started EdenFS" | tail -5
   ```

## Scenario: EdenFS is Down

The user reports EdenFS is not working.

### Step 1: Check current state
```bash
eden status --debug
```

Possible outcomes:
- **EdenFS running, service active** → working correctly, investigate user's actual problem
- **EdenFS not running, service failed** → go to Step 2
- **EdenFS not running, service inactive (dead)** → service was stopped cleanly or never started
- **EdenFS running, service dead** → mismatch (see next scenario)

### Step 2: Check why the service failed
```bash
# Check system messages for failure reason
grep 'edenfs@' /var/log/messages | tail -20

# Check the startup log
cat <state_dir>/.edenfs_startup.log
```

Look for:
- `code=killed, status=9/KILL` → OOM-killed
- `code=exited, status=1/FAILURE` → daemon exited with error
- `start operation timed out` → startup took longer than TimeoutStartSec (600s)
- `start-limit-hit` → exceeded StartLimitBurst (5 restarts in 120s)

### Step 3: If start-limit-hit, reset and restart
```bash
# Reset the failure counter
systemctl --user reset-failed edenfs@<instance>

# Then start
eden start
```

### Step 4: Check if systemd-managed is enabled
```bash
eden config | grep systemd-managed
```
If `false`, the daemon is running in legacy mode and systemd won't auto-restart it.

## Scenario: Systemd Service Failed but EdenFS Running

`eden status --debug` shows EdenFS running normally, but the systemd service is dead/failed.

This happens when:
- EdenFS was started outside of systemd (manual `edenfs` binary execution)
- A graceful restart failed — the old daemon kept running, the new one (started by systemd) crashed
- The service was stopped but a previous non-systemd daemon was still running

### Diagnosis
```bash
# Check the running process
eden status --debug

# Compare PIDs — which PID does systemd think it should track?
grep 'edenfs@' /var/log/messages | grep "Main PID" | tail -5

# Check eden version to see if upgrade was attempted
eden version
```

### Resolution
Usually, the running EdenFS is functional but won't auto-restart on crash. To re-sync:
```bash
eden restart
```
This stops the running daemon and starts a new one through systemd, restoring the managed lifecycle.

## Scenario: Restart Loop

Systemd keeps restarting EdenFS but it keeps failing.

### Detection
```bash
# Look for rapid restart counter increments
grep 'edenfs@.*restart counter' /var/log/messages | tail -10

# Example output showing a loop:
# Scheduled restart job, restart counter is at 106434.
```

### Diagnosis
```bash
# Check what error is causing restarts
grep 'edenfs@.*Failed with result' /var/log/messages | tail -5

# Check startup log for the repeated failure
cat <state_dir>/.edenfs_startup.log

# Check eden debug log for crash details
eden debug log | tail -100
```

### Common causes
- **ConfigeratorStaticData deadlock**: Look for `ConfigeratorStaticData::tryGetStatic() called on thread ... while CSD singleton factory is running`. This is a known startup race condition.
- **Missing dependencies**: `/dev/fuse` not available, missing packages
- **Corrupted state directory**: fsck running, incomplete previous shutdown
- **Stale takeover socket**: previous daemon didn't clean up socket, new daemon can't connect

### Resolution
If the service hit `StartLimitBurst`:
```bash
systemctl --user reset-failed edenfs@<instance>
eden start
```

## Scenario: Graceful Restart Failing

`eden restart --graceful` (systemctl reload) fails, usually during edenfs_upgrade.

### Symptoms
- Scuba shows `type=systemctl_action, action=reload, success=0`
- Old daemon keeps running with outdated version
- Error in startup log: `sendmsg() failed on UnixSocket: Transport endpoint is not connected`

### Diagnosis
```bash
# Check the startup log for takeover errors
cat <state_dir>/.edenfs_startup.log

# Check if old daemon is actually reachable
eden status

# Check version mismatch
eden version
```

### Common causes
- **Old daemon not listening on takeover socket**: crashed or wedged before the new daemon connected
- **Socket permission issues**: new daemon can't access the old daemon's socket
- **Timeout**: the old daemon is too busy (many checkouts) and the takeover takes too long

### Resolution
Fall back to a non-graceful restart:
```bash
eden restart
```

## Scenario: edenfs_upgrade Failures

The hourly upgrade timer triggers but edenfs fails to upgrade.

### Check timer status
```bash
# Timer schedule
systemctl list-timers '*edenfs_upgrade*'

# Timer details
systemctl status edenfs_upgrade.timer

# Service last run
systemctl status edenfs_upgrade.service
```

### Check edenfs_restarter logs
```bash
# Current log
tail -50 /var/facebook/logs/edenfs_upgrade.log

# Archived logs (if log was rotated)
ls -1 /var/facebook/logs/archive/edenfs_upgrade.log-*
zcat /var/facebook/logs/archive/edenfs_upgrade.log-YYYYMMDD.gz | tail -50
```

### View the service unit file
```bash
systemctl cat edenfs_upgrade.service
```

The service runs:
```
/usr/bin/systemd-run --scope /usr/local/libexec/eden/edenfs_restarter --max-age 7d --splay-time 2d --graceful
```

### Manually trigger an upgrade
```bash
sudo systemctl start edenfs_upgrade.service
```
This is equivalent to the timer-triggered run.

## Testing Changes Locally

When testing systemd-related changes on a devserver:

### 1. Build and install binaries
```bash
buck2 build '@fbcode//mode/opt' fbcode//eden/fs/cli_rs/edenfsctl:edenfsctl --out /tmp/built_edenfs_cli_rs
buck2 build '@fbcode//mode/opt' 'fbcode//eden/fs/cli:edenfsctl[standalone]' --out /tmp/built_edenfs_cli

# Backup originals
cp /usr/local/bin/edenfsctl /tmp/edenfsctl.bkp
cp /usr/local/bin/edenfsctl.real /tmp/edenfsctl.real.bkp

# Install
sudo cp /tmp/built_edenfs_cli_rs /usr/local/bin/edenfsctl
sudo cp /tmp/built_edenfs_cli /usr/local/bin/edenfsctl.real
```

### 2. Pause Chef
```bash
sudo /usr/facebook/ops/scripts/chef/stop_chef_temporarily -r "testing edenfs systemd changes" -t 1
```

### 3. Verify config
```bash
eden config | grep systemd-managed
```

### 4. Modify timer for faster iteration (optional)
```bash
# Backup
cp /etc/systemd/timers/edenfs_upgrade.timer /tmp/edenfs_upgrade.timer.bkp
cp /etc/systemd/timers/edenfs_upgrade.service /tmp/edenfs_upgrade.service.bkp

# Edit timer to run every 2 minutes with no splay
# In /etc/systemd/timers/edenfs_upgrade.timer:
#   OnCalendar=*:0/2
#   RandomizedDelaySec=0

# Edit service to force upgrade (ignore age/version checks)
# In /etc/systemd/timers/edenfs_upgrade.service, change ExecStart to:
#   ExecStart=/usr/bin/systemd-run --scope /usr/local/libexec/eden/edenfs_restarter -v --max-age 1m --min-uptime 1m --idle-threshold 1m --splay-time 1m --graceful --timeout 5m --skip-same-version-check

sudo systemctl daemon-reload
sudo systemctl restart edenfs_upgrade.timer
```

### 5. Monitor
```bash
# edenfs_upgrade log
tail -f /var/facebook/logs/edenfs_upgrade.log

# edenfs service log
grep "edenfs@" /var/log/messages | tail -f

# edenfs daemon logs
eden debug log
```

### 6. Restore after testing
```bash
sudo cp /tmp/edenfsctl.bkp /usr/local/bin/edenfsctl
sudo cp /tmp/edenfsctl.real.bkp /usr/local/bin/edenfsctl.real
sudo cp /tmp/edenfs_upgrade.timer.bkp /etc/systemd/timers/edenfs_upgrade.timer
sudo cp /tmp/edenfs_upgrade.service.bkp /etc/systemd/timers/edenfs_upgrade.service
sudo systemctl daemon-reload
sudo systemctl restart edenfs_upgrade.timer
```
