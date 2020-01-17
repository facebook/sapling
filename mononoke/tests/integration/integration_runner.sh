#!/usr/bin/env bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This script is provided as a compatibility shim for those who want to run
# individual tests, using Buck:
#
#   buck run scm/mononoke/tests/integration:integration_runner -- TEST

function protip() {
  local real_runner real_manifest
  real_runner="$(readlink "$1")"
  real_manifest="$(readlink "$2")"
  shift
  shift

  real_runner="$(readlink "$runner")"
  real_manifest="$(readlink "$manifest")"

  echo >&2
  echo "======" >&2
  echo "|| Pro tip: save time on your incremental test runs!" >&2
  echo "|| Run this command instead:" >&2
  echo "||" >&2
  echo "|| $real_runner $real_manifest $*" >&2
  echo "||" >&2
  echo "|| Between test runs, manually rebuild only the binaries you care about using buck build." >&2
  echo "|| Check out the README.md for integration tests to learn more." >&2
  echo "======"
  echo >&2
}

d="$BUCK_DEFAULT_RUNTIME_RESOURCES"
runner="${d}/integration_runner_real"
manifest="${d}/manifest.json"

protip "$runner" "$manifest" "$@"
exec "$runner" "$manifest" "$@"
