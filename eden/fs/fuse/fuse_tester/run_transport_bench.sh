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

main() {
  if [[ "${1:-}" == "--help" ]]; then
    usage
    return 0
  fi

  require_command buck2
  require_command pidstat
  require_command pgrep
  require_command python3
  require_command /usr/bin/time

  validate_configuration
  print_configuration
  configure_mode devfuse
  configure_mode io_uring
}

main "$@"
