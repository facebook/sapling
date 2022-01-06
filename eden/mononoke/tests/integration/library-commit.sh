#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Library routines for generating or interacting with commits

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"

function whereami() {
  hg log -r . -T "{node}"
}

function add_triangle_merge_commits_and_push() {
  # Add multiple merge commits in a triangle like shape:
  # o
  # |\
  # o o
  # | |\
  # o o o
  # | |/
  # o o
  # |/
  # o
  # The first argument represents how many merges to add.
  # The second argument is optional and represents a unique identifier for the
  # current invocation of the function.
  count="$1"
  shift
  local seed tomerge topush st commit i j
  seed=${1:-$(wherami)}
  for i in $(seq 1 "$count"); do
    echo "i=${i}" >> "${seed}_${i}"
    hg ci -Am "${seed}: base branch ${i}"
  done
  tomerge="$(whereami)"
  topush=()
  for i in $(seq 1 "$count"); do
    hg prev
    st="$(whereami)"
    for j in $(seq 1 $((2 * i - 1))); do
      echo "j=$j" >> "${seed}_${i}_${j}"
      hg ci -Am "${seed}: commit ${j} for branch ${i}"
      if [ "${i}" == "${count}" ]; then
        topush+=( "$(whereami)" )
      fi
    done
    hg merge -r "${tomerge}"
    hg commit -Am "${seed}: merge branch ${i}"
    tomerge="$(whereami)"
    hg co -r "${st}"
  done
  topush+=( "$tomerge" )

  # simulate updating master with several single parents commits then a merge
  for commit in "${topush[@]}"; do
    hg co "${commit}"
    hgmn push -r . --to master_bookmark --pushvar NON_FAST_FORWARD=true
  done
}
