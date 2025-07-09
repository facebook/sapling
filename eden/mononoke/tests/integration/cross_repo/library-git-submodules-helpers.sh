#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Store generic helper functions related for cross-repo sync with git submodules
# integration tests, e.g. create commits, clone repos, print logs, etc


# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"

# Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
export XDG_CONFIG_HOME=$TESTTMP
git config --global protocol.file.allow always


function print_section() {
    printf "\n\nNOTE: %s\n" "$1"
}

# Helper that takes a message and a file and creates a git commit
function mk_git_commit() {
  file=${2-file}
  echo "$1" > "$file"
  git add "$file"
  git commit -aqm "$1"
}

# Helper that takes a message and a file and creates a sapling commit
function mk_sl_commit() {
  echo "$1" > "${2-file}"
  sl commit -Aq -m "$1"
}

function sl_log() {
   hg log --graph -T '{node|short} {desc}\n' "$@"
}

function clone_and_log_large_repo {
  LARGE_BCS_IDS=( "$@" )
  cd "$TESTTMP" || exit
  clone_large_repo

  cd "$LARGE_REPO_NAME" || exit
  enable commitcloud infinitepush # to push commits to server

  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    LARGE_CS_ID=$(mononoke_admin convert --from bonsai --to hg -R "$LARGE_REPO_NAME" "$LARGE_BCS_ID" --derive)
    if [ -n "$LARGE_CS_ID" ]; then
      hg pull -q -r "$LARGE_CS_ID"
    fi
  done

  sl_log --stat -r "sort(all(), desc)"

  printf "\n\nRunning mononoke_admin to verify mapping\n\n"
  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    quiet_grep RewrittenAs -- mononoke_admin cross-repo --source-repo-id "$LARGE_REPO_ID" --target-repo-id "$SUBMODULE_REPO_ID" map -i "$LARGE_BCS_ID"
  done

  printf "\nDeriving all the enabled derived data types\n"
  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    quiet mononoke_admin derived-data -R "$LARGE_REPO_NAME" derive --all-types \
      -i "$LARGE_BCS_ID" 2>&1| rg "Error" || true # filter to keep only Error line if there is an error
  done
}

# Clone the large repo if it hasn't been cloned yet
function clone_large_repo {
  orig_pwd=$(pwd)
  cd "$TESTTMP" || exit
  if [ ! -d "$LARGE_REPO_NAME" ]; then
    hg clone -q "mono:$LARGE_REPO_NAME" "$LARGE_REPO_NAME"
  fi

  cd "$orig_pwd" || exit
}
