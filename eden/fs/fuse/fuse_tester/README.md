# `fuse_tester`

This directory contains local tools for exercising and benchmarking the EdenFS
FUSE transport.

`run_transport_bench.sh` compares `devfuse` and `io_uring` by:
- recreating the Eden mount in each mode
- running a filesystem workload
- collecting client-side `/usr/bin/time` output
- collecting EdenFS daemon CPU from `/proc/$PID/stat`
- printing a compact comparison summary

Optional cold-cache mode:
- set `DROP_CACHES=1` to clear Linux kernel caches when switching between modes
- `DROP_CACHES_MODE=2` clears dentries/inodes
- `DROP_CACHES_MODE=3` clears page cache, dentries, and inodes
- cache dropping is host-wide and requires `sudo`

Default workload:
- recursive `ls -lR > /dev/null` under `TARGET_DIR`

Available workloads:
- `ls_recursive`
- `tar_recursive`
- `rg_recursive`

`tar_recursive` runs `tar -cf /dev/null .` over the target subtree and is useful
when you want a cleaner read-heavy macro workload than `rg_recursive`.

`rg_recursive` runs ripgrep over the target subtree and is useful when you want
to stress file reads in addition to traversal.

Example:

```bash
cd fbcode
chmod +x eden/fs/fuse/fuse_tester/run_transport_bench.sh
TARGET_DIR="$HOME/fbsource-dev/fbcode/eden" \
RUNS=5 \
eden/fs/fuse/fuse_tester/run_transport_bench.sh
```

Cold-cache example:

```bash
cd fbcode
chmod +x eden/fs/fuse/fuse_tester/run_transport_bench.sh
TARGET_DIR="$HOME/fbsource-dev/fbcode/eden" \
DROP_CACHES=1 \
DROP_CACHES_MODE=3 \
RUNS=5 \
eden/fs/fuse/fuse_tester/run_transport_bench.sh
```

Tar example:

```bash
cd fbcode
chmod +x eden/fs/fuse/fuse_tester/run_transport_bench.sh
WORKLOAD=tar_recursive \
TARGET_DIR="$HOME/fbsource-dev/fbcode/eden" \
DROP_CACHES=1 \
DROP_CACHES_MODE=3 \
RUNS=5 \
eden/fs/fuse/fuse_tester/run_transport_bench.sh
```


Ripgrep example:

```bash
cd fbcode
chmod +x eden/fs/fuse/fuse_tester/run_transport_bench.sh
WORKLOAD=rg_recursive \
TARGET_DIR="$HOME/fbsource-dev/fbcode/eden" \
DROP_CACHES=1 \
DROP_CACHES_MODE=3 \
RG_JOBS=117 \
RG_PATTERN="" \
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
- `WORKLOAD`
- `RG_JOBS`
- `RG_PATTERN`
- `DROP_CACHES`
- `DROP_CACHES_MODE`
- `OUTPUT_DIR`
- `DRY_RUN`
