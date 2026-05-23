#!/usr/bin/env bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -euo pipefail

EDEN_DEV_STATE="${EDEN_DEV_STATE:-$HOME/local/eden-dev-state}"
MOUNT_DIR="${MOUNT_DIR:-$HOME/fbsource-dev}"
BACKING_REPO="${BACKING_REPO:-$HOME/local/.eden-backing-repos/fbsource}"
CLONE_REVISION="${CLONE_REVISION:-master}"
TARGET_DIR="${TARGET_DIR:-$MOUNT_DIR/fbcode/eden}"
RUNS="${RUNS:-5}"
WORKLOAD="${WORKLOAD:-ls_recursive}"
RG_JOBS="${RG_JOBS:-117}"
RG_PATTERN="${RG_PATTERN:-}"
DROP_CACHES="${DROP_CACHES:-0}"
DROP_CACHES_MODE="${DROP_CACHES_MODE:-3}"
DRY_RUN="${DRY_RUN:-0}"
OUTPUT_DIR="${OUTPUT_DIR:-/tmp}"

usage() {
  cat <<EOF
Usage: $(basename "$0")

Environment overrides:
  EDEN_DEV_STATE   Eden config dir
  MOUNT_DIR        Eden mount path
  BACKING_REPO     Backing repo used for reclone
  CLONE_REVISION   Revision passed to edenfsctl clone
  TARGET_DIR       Subdirectory used by the workload
  RUNS             Number of benchmark repetitions per mode
  WORKLOAD         Benchmark workload name (ls_recursive or rg_recursive)
  RG_JOBS          ripgrep -j value when WORKLOAD=rg_recursive
  RG_PATTERN       ripgrep search pattern when WORKLOAD=rg_recursive
  DROP_CACHES      Drop Linux kernel caches when switch between modes (0 or 1)
  DROP_CACHES_MODE Linux drop_caches mode (2 for dentries/inodes, 3 for all)
  DRY_RUN          Print Eden commands instead of executing them (0 or 1)
  OUTPUT_DIR       Directory for raw benchmark outputs
EOF
}

require_command() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 1
  fi
}

validate_configuration() {
  if [[ "$RUNS" -le 0 ]]; then
    echo "RUNS must be positive" >&2
    exit 1
  fi

  case "$WORKLOAD" in
    ls_recursive|rg_recursive) ;;
    *)
      echo "Unsupported WORKLOAD: $WORKLOAD" >&2
      exit 1
      ;;
  esac

  if [[ "$RG_JOBS" -le 0 ]]; then
    echo "RG_JOBS must be positive" >&2
    exit 1
  fi

  if [[ "$DROP_CACHES" != "0" && "$DROP_CACHES" != "1" ]]; then
    echo "DROP_CACHES must be 0 or 1" >&2
    exit 1
  fi

  case "$DROP_CACHES_MODE" in
    2|3) ;;
    *)
      echo "DROP_CACHES_MODE must be 2 or 3" >&2
      exit 1
      ;;
  esac

  if [[ "$DRY_RUN" != "0" && "$DRY_RUN" != "1" ]]; then
    echo "DRY_RUN must be 0 or 1" >&2
    exit 1
  fi

  mkdir -p "$OUTPUT_DIR"
}

print_configuration() {
  cat <<EOF
Transport benchmark configuration
  EDEN_DEV_STATE: $EDEN_DEV_STATE
  MOUNT_DIR:      $MOUNT_DIR
  BACKING_REPO:   $BACKING_REPO
  CLONE_REVISION: $CLONE_REVISION
  TARGET_DIR:     $TARGET_DIR
  RUNS:           $RUNS
  WORKLOAD:       $WORKLOAD
  RG_JOBS:        $RG_JOBS
  RG_PATTERN:     ${RG_PATTERN:-<empty>}
  DROP_CACHES:    $DROP_CACHES
  DROP_CACHES_MODE: $DROP_CACHES_MODE
  DRY_RUN:        $DRY_RUN
  OUTPUT_DIR:     $OUTPUT_DIR
EOF
}

run_cmd() {
  echo "+ $*"
  if [[ "$DRY_RUN" == "1" ]]; then
    return 0
  fi
  "$@"
}

run_optional_cmd() {
  if ! run_cmd "$@"; then
    echo "warning: command failed but benchmark will continue: $*" >&2
  fi
}

drop_kernel_caches() {
  if [[ "$DROP_CACHES" != "1" ]]; then
    return 0
  fi

  run_cmd sync
  run_cmd sudo sh -c "echo $DROP_CACHES_MODE > /proc/sys/vm/drop_caches"
}

reclone_mount() {
  run_optional_cmd buck2 run @mode/opt edenfsctl -- --config-dir="$EDEN_DEV_STATE" stop
  run_optional_cmd buck2 run @mode/opt edenfsctl -- --config-dir="$EDEN_DEV_STATE" rm "$MOUNT_DIR"
  run_cmd buck2 run @mode/opt edenfsctl -- --config-dir="$EDEN_DEV_STATE" start
  run_cmd buck2 run @mode/opt edenfsctl -- --config-dir="$EDEN_DEV_STATE" clone \
    "$BACKING_REPO" -r "$CLONE_REVISION" "$MOUNT_DIR"
}

configure_mode() {
  local mode="$1"

  cat <<EOF

Configure ~/.edenrc for mode: $mode
  devfuse  -> use-io-uring = "false"
  io_uring -> use-io-uring = "true"
EOF
  read -r -p "Press Enter when $mode is configured..."
  reclone_mount
  echo "Mount prepared for mode: $mode"
}

run_workload() {
  case "$WORKLOAD" in
    ls_recursive)
      (
        cd "$TARGET_DIR"
        /usr/bin/time -f 'real_sec=%e user_sec=%U sys_sec=%S cpu_pct=%P' \
          sh -c 'ls -lR > /dev/null'
      )
      ;;
    rg_recursive)
      (
        cd "$TARGET_DIR"
        /usr/bin/time -f 'real_sec=%e user_sec=%U sys_sec=%S cpu_pct=%P' \
          rg -j "$RG_JOBS" "$RG_PATTERN" > /dev/null
      )
      ;;
  esac
}

find_edenfs_pid() {
  buck2 run @mode/opt edenfsctl -- --config-dir="$EDEN_DEV_STATE" pid
}

get_clock_ticks_per_sec() {
  getconf CLK_TCK
}

read_edenfs_cpu_ticks() {
  local pid="$1"
  python3 - "$pid" <<'PY'
import sys

pid = sys.argv[1]
with open(f"/proc/{pid}/stat", "r", encoding="utf-8") as f:
    stat = f.read().strip()

fields = stat.rsplit(") ", 1)[1].split()
utime = int(fields[11])
stime = int(fields[12])
print(f"{utime} {stime}")
PY
}

compute_edenfs_cpu_sample() {
  local before_utime="$1"
  local before_stime="$2"
  local after_utime="$3"
  local after_stime="$4"
  local clock_ticks="$5"
  local real_sec="$6"

  python3 - \
    "$before_utime" \
    "$before_stime" \
    "$after_utime" \
    "$after_stime" \
    "$clock_ticks" \
    "$real_sec" <<'PY'
import sys

before_utime = int(sys.argv[1])
before_stime = int(sys.argv[2])
after_utime = int(sys.argv[3])
after_stime = int(sys.argv[4])
clock_ticks = float(sys.argv[5])
real_sec = float(sys.argv[6])

user_sec = (after_utime - before_utime) / clock_ticks
sys_sec = (after_stime - before_stime) / clock_ticks
cpu_sec = user_sec + sys_sec
cpu_pct = (cpu_sec / real_sec) * 100.0 if real_sec > 0 else 0.0

print(
    f"edenfs_user_sec={user_sec:.6f} "
    f"edenfs_sys_sec={sys_sec:.6f} "
    f"edenfs_cpu_sec={cpu_sec:.6f} "
    f"edenfs_cpu_pct={cpu_pct:.3f}"
)
PY
}

extract_real_sec() {
  local workload_output="$1"
  python3 - "$workload_output" <<'PY'
import re
import sys

text = sys.argv[1]
match = re.search(r"real_sec=(\S+)", text)
if not match:
    raise SystemExit("failed to parse real_sec from workload output")
print(match.group(1))
PY
}

run_mode() {
  local mode="$1"
  local raw_out="$OUTPUT_DIR/transport-bench-${mode}.txt"
  local eden_cpu_out="$OUTPUT_DIR/transport-bench-${mode}-edenfs-cpu.txt"
  local clock_ticks
  clock_ticks="$(get_clock_ticks_per_sec)"

  : > "$raw_out"
  : > "$eden_cpu_out"

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "DRY_RUN: skipping workload for mode=$mode" | tee -a "$raw_out"
    echo "DRY_RUN: skipping edenfs CPU capture for mode=$mode" \
      | tee -a "$eden_cpu_out"
    return 0
  fi

  local eden_pid
  eden_pid="$(find_edenfs_pid)"
  if [[ -z "$eden_pid" ]]; then
    echo "Failed to find edenfs pid for mode=$mode" >&2
    exit 1
  fi

  drop_kernel_caches
  for run in $(seq 1 "$RUNS"); do
    local before_utime
    local before_stime
    read -r before_utime before_stime < <(read_edenfs_cpu_ticks "$eden_pid")

    echo "mode=$mode run=$run target=$TARGET_DIR" | tee -a "$raw_out"
    local workload_output
    workload_output="$(run_workload 2>&1)"
    printf '%s\n' "$workload_output" | tee -a "$raw_out"

    local after_utime
    local after_stime
    read -r after_utime after_stime < <(read_edenfs_cpu_ticks "$eden_pid")

    local real_sec
    real_sec="$(extract_real_sec "$workload_output")"
    {
      echo "mode=$mode run=$run pid=$eden_pid target=$TARGET_DIR"
      compute_edenfs_cpu_sample \
        "$before_utime" \
        "$before_stime" \
        "$after_utime" \
        "$after_stime" \
        "$clock_ticks" \
        "$real_sec"
      echo
    } | tee -a "$eden_cpu_out"

    echo >>"$raw_out"
  done
}

print_comparison_summary() {
  python3 - \
    "$OUTPUT_DIR/transport-bench-devfuse.txt" \
    "$OUTPUT_DIR/transport-bench-io_uring.txt" \
    "$OUTPUT_DIR/transport-bench-devfuse-edenfs-cpu.txt" \
    "$OUTPUT_DIR/transport-bench-io_uring-edenfs-cpu.txt" <<'PY'
import re
import sys

dev_raw, uring_raw, dev_eden_cpu_path, uring_eden_cpu_path = sys.argv[1:]

time_line = re.compile(
    r"real_sec=(?P<real>\S+)\s+user_sec=(?P<user>\S+)\s+sys_sec=(?P<sys>\S+)\s+cpu_pct=(?P<cpu>\S+)"
)
eden_cpu_line = re.compile(
    r"edenfs_user_sec=(?P<user>\S+)\s+"
    r"edenfs_sys_sec=(?P<sys>\S+)\s+"
    r"edenfs_cpu_sec=(?P<cpu_sec>\S+)\s+"
    r"edenfs_cpu_pct=(?P<cpu_pct>\S+)"
)


def parse_workload(path):
    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            m = time_line.search(line)
            if not m:
                continue
            rows.append(
                {
                    "real": float(m.group("real")),
                    "user": float(m.group("user")),
                    "sys": float(m.group("sys")),
                    "cpu": float(m.group("cpu").rstrip("%")),
                }
            )
    return rows


def parse_eden_cpu(path):
    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            m = eden_cpu_line.search(line)
            if not m:
                continue
            rows.append(
                {
                    "user": float(m.group("user")),
                    "sys": float(m.group("sys")),
                    "cpu_sec": float(m.group("cpu_sec")),
                    "cpu_pct": float(m.group("cpu_pct")),
                }
            )
    return rows


def mean(values):
    return sum(values) / len(values) if values else None


def pct_change(base, new):
    if base in (None, 0) or new is None:
        return None
    return ((new - base) / base) * 100.0


def fmt_num(value):
    return f"{value:.3f}" if value is not None else "n/a"


def fmt_pct(value):
    return f"{value:+.1f}%" if value is not None else "n/a"


dev_workload = parse_workload(dev_raw)
uring_workload = parse_workload(uring_raw)
dev_eden_cpu = parse_eden_cpu(dev_eden_cpu_path)
uring_eden_cpu = parse_eden_cpu(uring_eden_cpu_path)

if not dev_workload:
    raise SystemExit(f"no workload samples captured in {dev_raw}")
if not uring_workload:
    raise SystemExit(f"no workload samples captured in {uring_raw}")
if not dev_eden_cpu:
    raise SystemExit(f"no edenfs CPU samples captured in {dev_eden_cpu_path}")
if not uring_eden_cpu:
    raise SystemExit(f"no edenfs CPU samples captured in {uring_eden_cpu_path}")

metrics = [
    ("real", "real_sec"),
    ("user", "user_sec"),
    ("sys", "sys_sec"),
    ("cpu", "client_cpu_pct"),
]

print()
print("Workload comparison")
print(f"{'metric':>16}  {'devfuse':>10}  {'io_uring':>10}  {'delta %':>8}")
print("-" * 52)
for key, label in metrics:
    dev_mean = mean([row[key] for row in dev_workload])
    uring_mean = mean([row[key] for row in uring_workload])
    delta = pct_change(dev_mean, uring_mean)
    print(
        f"{label:>16}  {fmt_num(dev_mean):>10}  {fmt_num(uring_mean):>10}  {fmt_pct(delta):>8}"
    )

print()
print("EdenFS CPU comparison")
print(f"{'metric':>16}  {'devfuse':>10}  {'io_uring':>10}  {'delta %':>8}")
print("-" * 52)
for key, label in [
    ("user", "edenfs_user_sec"),
    ("sys", "edenfs_sys_sec"),
    ("cpu_sec", "edenfs_cpu_sec"),
    ("cpu_pct", "edenfs_cpu_pct"),
]:
    dev_mean = mean([row[key] for row in dev_eden_cpu])
    uring_mean = mean([row[key] for row in uring_eden_cpu])
    print(
        f"{label:>16}  {fmt_num(dev_mean):>10}  {fmt_num(uring_mean):>10}  "
        f"{fmt_pct(pct_change(dev_mean, uring_mean)):>8}"
    )

print()
print("Interpretation:")
print("- Lower real_sec is better for end-to-end latency.")
print("- Lower user_sec/sys_sec is better for client-side CPU cost.")
print("- edenfs_* metrics come from /proc/$PID/stat over the exact benchmark window.")
PY
}

main() {
  if [[ "${1:-}" == "--help" ]]; then
    usage
    return 0
  fi

  require_command buck2
  require_command python3
  if [[ "$WORKLOAD" == "rg_recursive" ]]; then
    require_command rg
  fi
  require_command /usr/bin/time

  validate_configuration
  print_configuration
  configure_mode devfuse
  run_mode devfuse
  configure_mode io_uring
  run_mode io_uring

  cat <<EOF

Raw outputs:
  $OUTPUT_DIR/transport-bench-devfuse.txt
  $OUTPUT_DIR/transport-bench-devfuse-edenfs-cpu.txt
  $OUTPUT_DIR/transport-bench-io_uring.txt
  $OUTPUT_DIR/transport-bench-io_uring-edenfs-cpu.txt
EOF

  if [[ "$DRY_RUN" == "1" ]]; then
    echo "DRY_RUN: skipping comparison summary"
  else
    print_comparison_summary
  fi
}

main "$@"
