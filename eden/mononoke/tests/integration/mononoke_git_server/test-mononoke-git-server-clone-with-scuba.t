# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ SCUBA="$TESTTMP/scuba.json"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# A single Git clone involves three requests to the Git server. Check if we
# have 3 scuba records corresponding to the request
  $ cat "$SCUBA" | grep "MononokeGit Request Processed" | wc -l
  3

# Verify the stream statistics get recorded in scuba
  $ jq -S .int "$SCUBA" | grep stream
    "stream_completed": *, (glob)
    "stream_completion_time_us": *, (glob)
    "stream_count": *, (glob)
    "stream_first_item_time_us": *, (glob)
    "stream_max_poll_time_us": *, (glob)
    "stream_poll_count": *, (glob)
    "stream_poll_time_us": *, (glob)

# Verify the future statistics get recorded in scuba
  $ jq -S .int "$SCUBA" | grep [^_]poll
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)
