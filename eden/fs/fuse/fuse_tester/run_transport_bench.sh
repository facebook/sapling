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
  WORKLOAD         Benchmark workload name (currently: ls_recursive)
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
    ls_recursive) ;;
    *)
      echo "Unsupported WORKLOAD: $WORKLOAD" >&2
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

main() {
  if [[ "${1:-}" == "--help" ]]; then
    usage
    return 0
  fi

  require_command buck2
  require_command python3
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
}

main "$@"
