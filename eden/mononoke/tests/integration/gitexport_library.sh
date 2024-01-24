#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Initial setup for gitexport integration tests.

. "${TEST_FIXTURES}/library.sh"

# Setup configuration
REPOTYPE="blob_files"
setup_common_config "$REPOTYPE"
ENABLE_API_WRITES=1 REPOID=1 setup_mononoke_repo_config "temp_repo"

HG_REPO="$TESTTMP/repo"
SOURCE_REPO_LOG="$TESTTMP/source_repo_log"
GIT_BUNDLE_OUTPUT="$TESTTMP/git_bundle"
GIT_REPO="$TESTTMP/git_repo"
GIT_REPO_LOG="$TESTTMP/git_repo_log"
SCUBA_LOGS_FILE="file://$TESTTMP/scuba_gitexport_logs"

# Call gitexport with the proper arguments for all integration tests
function test_gitexport {
  gitexport --repo-name "repo" -B "master" --scuba-dataset="$SCUBA_LOGS_FILE" --git-output "$GIT_BUNDLE_OUTPUT" "$@"
}


# Run the log command on both repos applying sed transformations to have the same
# format on both outputs.
# Then compare the outputs using diff to see what commits and/or file changes
# were synced from the source hg repo to the git repo.
function diff_hg_and_git_repos {
  cd "$HG_REPO" || exit
  hg log --git --template "{firstline(desc)}\n{stat()}\n" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > "$SOURCE_REPO_LOG"

  cd "$GIT_REPO" || exit
  git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > "$GIT_REPO_LOG"

  diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  cd "$TESTTMP" || exit

  # Delete files in case we run gitexport again
  rm -rf "$GIT_BUNDLE_OUTPUT" "$GIT_REPO" "$GIT_REPO_LOG"
}
