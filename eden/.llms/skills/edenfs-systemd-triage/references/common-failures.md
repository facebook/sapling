# Common Failure Patterns

## Table of Contents
- [OOM Kill](#oom-kill)
- [Start Timeout](#start-timeout)
- [Start Limit Exceeded](#start-limit-exceeded)
- [Graceful Restart Takeover Failure](#graceful-restart-takeover-failure)
- [ConfigeratorStaticData Deadlock](#configeratorstaticdata-deadlock)
- [Service Dead but EdenFS Running](#service-dead-but-edenfs-running)
- [D-Bus / XDG_RUNTIME_DIR Missing](#d-bus--xdg_runtime_dir-missing)
- [Unit File Permission Warning](#unit-file-permission-warning)
- [Scribe Category Not Configured](#scribe-category-not-configured)
- [Empty Error Field in Scuba](#empty-error-field-in-scuba)

---

## OOM Kill

### Signature
```
edenfs@<instance>.service: Main process exited, code=killed, status=9/KILL
edenfs@<instance>.service: Failed with result 'signal'.
```

### Root Cause
The edenfs daemon was killed by the kernel OOM killer or fb-oomd. Common on devservers with heavy buck2 usage competing for memory.

### Diagnosis
```bash
# Check system messages for OOM
grep -i 'oom\|out of memory\|killed process' /var/log/messages | tail -20

# Check cgroup memory usage
cat /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/edenfs.slice/memory.current
cat /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/edenfs.slice/memory.high
```

### Resolution
Systemd should auto-restart after 5 seconds (`Restart=on-failure`). If it doesn't:
- Check if `StartLimitBurst` was exceeded
- Check if memory protection (`MemoryLow`, `MemoryHigh`) is configured on the `edenfs.slice`

### Remaining child processes
After an OOM kill, `KillMode=process` means only the main edenfs process is killed. Orphaned processes may remain:
```
Unit process <PID> (edenfs_privhelp) remains running after unit stopped.
Unit process <PID> (scribe_cat) remains running after unit stopped.
```
These are cleaned up when the new daemon starts, or by the system eventually.

---

## Start Timeout

### Signature
```
edenfs@<instance>.service: start operation timed out. Terminating.
```

### Root Cause
EdenFS startup took longer than `TimeoutStartSec=600` (10 minutes). This happens with:
- Many checkouts requiring fsck
- Slow disk I/O
- Large overlay data to replay

### Diagnosis
```bash
# Check eden debug log for slow startup phases
eden debug log | grep -E "fsck|OverlayChecker|scan [0-9]+%" | tail -20

# Check startup log for timing
cat <state_dir>/.edenfs_startup.log
```

### Resolution
The timeout is 600 seconds. If EdenFS routinely needs longer (e.g., 50+ checkouts), the unit file timeout may need increasing. Check if the daemon actually started successfully despite the timeout — systemd may have killed a daemon that was about to finish.

---

## Start Limit Exceeded

### Signature
```
edenfs@<instance>.service: start-limit-hit
Failed to start edenfs@<instance>.service
```

### Root Cause
More than 5 start attempts within 120 seconds (`StartLimitBurst=5`, `StartLimitIntervalSec=120`). Usually means the daemon is crashing immediately on startup.

### Diagnosis
```bash
# Check what's causing repeated startup failures
cat <state_dir>/.edenfs_startup.log
eden debug log | grep "error starting EdenFS" | tail -5
```

### Resolution
```bash
# Reset the failure counter
systemctl --user reset-failed edenfs@<instance>

# Fix the underlying issue, then restart
eden start
```

---

## Graceful Restart Takeover Failure

### Signature
In startup log:
```
error starting EdenFS: std::system_error: sendmsg() failed on UnixSocket: Transport endpoint is not connected
```

Or in eden debug log:
```
error receiving takeover data: std::system_error: sendmsg() failed on UnixSocket: Transport endpoint is not connected
```

### Root Cause
The new daemon tried to connect to the old daemon's takeover socket, but the old daemon had already crashed or wasn't listening. This often happens when:
- The old daemon crashed between the reload request and the takeover attempt
- The old daemon is wedged and not responding
- The old daemon version is too old and doesn't support takeover properly

### Diagnosis
```bash
# Check the old daemon's state
eden debug log | grep -B10 "sendmsg.*Transport endpoint"

# Check if old daemon crashed
grep 'edenfs@.*exited' /var/log/messages | tail -5

# Check version mismatch
eden version
```

### Resolution
Fall back to non-graceful restart:
```bash
eden restart
```

---

## ConfigeratorStaticData Deadlock

### Signature
```
ConfigeratorStaticData::tryGetStatic() called on thread <X> while CSD singleton factory is running on thread <Y>.
This call will block on CSD's mutex and may deadlock if CSD's factory transitively needs a singleton this thread is constructing.
```

### Root Cause
A startup race condition in the Configerator initialization code. The ThriftServer construction triggers a config fetch on a thread while the CSD singleton is being initialized on another thread.

### Diagnosis
Check the startup log or eden debug log for the full stack trace. The crash typically happens during `EdenServer::createThriftServer()` → `ConfigeratorApi::getStaticData()`.

### Resolution
This is a known issue (see T258664363). The daemon usually crashes and systemd restarts it. If it keeps happening, the restart should eventually succeed when the race doesn't trigger. If stuck in a restart loop, try:
```bash
systemctl --user reset-failed edenfs@<instance>
eden start
```

---

## Service Dead but EdenFS Running

### Signature
`eden status --debug` shows:
```
EdenFS is running normally (pid XXXXX)

x edenfs@<instance>.service - EdenFS (state directory: ...)
     Active: failed (Result: exit-code) since ...
```

### Root Cause
The systemd-managed daemon failed, but a legacy (non-systemd) daemon is still running. Or a graceful restart partially failed — old daemon survived, new one didn't.

### Resolution
Restart to re-sync systemd state:
```bash
eden restart
```

---

## D-Bus / XDG_RUNTIME_DIR Missing

### Signature
`systemctl --user` commands fail with:
```
Failed to connect to bus: No medium found
```

### Root Cause
The user's D-Bus session is not available. Happens when:
- SSH session doesn't have the user's systemd session context
- `sush` to the host without `machinectl shell`
- `XDG_RUNTIME_DIR` is not set

### Diagnosis
```bash
# Check if D-Bus socket exists
ls -la /run/user/$(id -u)/bus

# Check XDG_RUNTIME_DIR
echo $XDG_RUNTIME_DIR
```

### Resolution
Set `XDG_RUNTIME_DIR` manually:
```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
```

Or use `machinectl shell` which properly enters the user's session.

---

## Unit File Permission Warning

### Signature
```
Configuration file /usr/lib/systemd/user/edenfs@.service is marked executable.
Please remove executable permission bits. Proceeding anyway.
```

### Root Cause
The unit file was deployed with executable permissions. Systemd warns but continues.

### Resolution
Harmless warning. Fix by:
```bash
sudo chmod 644 /usr/lib/systemd/user/edenfs@.service
```

---

## Scribe Category Not Configured

### Signature
In startup log:
```
Scribe binary '/usr/local/bin/scribe_cat' specified, but no category specified. Structured logging is disabled.
```

### Root Cause
EdenFS was started without the scribe category configuration. Structured logging to Scuba is disabled for this daemon instance.

### Resolution
This is typically a configuration issue in the args file. Check `.edenfs_start_args` to verify the correct arguments are being passed.

---

## Empty Error Field in Scuba

### Signature
Scuba shows `systemctl_action` events with `success=0` but `error=null`.

### Root Cause
Some errors are logged to the startup log file (`<state_dir>/.edenfs_startup.log`) but not captured in the Scuba event. The error capture in the CLI may not cover all failure paths.

### Diagnosis
Always check the startup log on the host when Scuba shows an empty error:
```bash
cat <state_dir>/.edenfs_startup.log
```

### Resolution
This is a known gap. D104740191 tracks improving error capture. For now, rely on host-side logs for the full picture.

## References

- Design doc: https://docs.google.com/document/d/16U6n0u44qobkWOFoHpcCSrsOy5-X4LxsORIae4VQ2cI/edit
- Wiki: https://www.internalfb.com/wiki/Source_Control/EdenFS/Hacking_on_Eden/Systemd-Managed_EdenFS/
- Triage notes: https://docs.google.com/document/d/164j186KgVF_bLbJslgCqkc-w5APjZyY5b3Zsksuxe6E/edit
- Task: T258327606
