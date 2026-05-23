# `fuse_tester`

This directory contains local tools for exercising and benchmarking the EdenFS
FUSE transport.

`run_transport_bench.sh` compares `devfuse` and `io_uring` by:
- recreating the Eden mount in each mode
- running a filesystem workload
- collecting client-side `/usr/bin/time` output
- collecting EdenFS daemon CPU from `/proc/$PID/stat`
- printing a compact comparison summary

Default workload:
- recursive `ls -lR > /dev/null` under `TARGET_DIR`

Example:

```bash
cd fbcode
chmod +x eden/fs/fuse/fuse_tester/run_transport_bench.sh
TARGET_DIR="$HOME/fbsource-dev/fbcode/eden" \
RUNS=5 \
eden/fs/fuse/fuse_tester/run_transport_bench.sh
```

Useful environment overrides:
- `EDEN_DEV_STATE`
- `MOUNT_DIR`
- `BACKING_REPO`
- `CLONE_REVISION`
- `TARGET_DIR`
- `RUNS`
- `OUTPUT_DIR`
- `DRY_RUN`
