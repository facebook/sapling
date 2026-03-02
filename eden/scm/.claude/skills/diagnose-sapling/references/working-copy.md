# Working Copy Diagnosis

**Working copy problems — files on disk, file states, checkout**

## Run these first

```bash
eden status                  # is EdenFS running? shows pid
eden list --json             # all mounts with state (RUNNING/NOT_RUNNING) and backing repo paths
eden doctor                  # 25+ health checks with auto-repair
sl doctor                    # repairs treestate, metalog, segments
```

## Then check blackbox for relevant events

```bash
# Check watchman health (drives sl status performance)
sl blackbox --start 60 --pattern '{"watchman": "_"}'

# Check fsmonitor events
sl blackbox --start 60 --pattern '{"fsmonitor": "_"}'

# Check for blocked operations (waiting on editor, hook, etc.)
sl blackbox --start 60 --pattern '{"blocked": "_"}'

# Find slow commands in the last 24 hours
sl blackbox --start 1440 --pattern '{"finish": {"duration_ms": ["range", 10000, 999999]}}'
```

## Common working copy problems

- **`sl status` slow** — check watchman events in blackbox, restart watchman if stale: `watchman watch-del-all && watchman shutdown-server`
- **Files missing or wrong on disk** — `eden doctor` to check mount and overlay health
- **Checkout/update failures** — check EdenFS health with `eden status`, check blackbox for fetch stats (scmstore section in metrics JSON)
- **I/O errors** — `eden doctor` to check mount, may need `eden unmount <path> && eden mount <path>`
- **Disk space issues** — see [edenfs-diagnosis.md](edenfs-diagnosis.md) disk space section
- **Certificate errors** — `eden doctor` checks certs at `/var/facebook/credentials/$USER/x509/$USER.pem`; fix with `update-certificates`
- **EdenFS high memory** — `eden stats` for inode count, `eden gc` to unload unused inodes
- **Corrupt treestate** — `sl doctor` auto-repairs
