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
EOF
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
}

main "$@"
