# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ SCUBA="$TESTTMP/scuba.json"

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
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

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
  $ jq -S .int "$SCUBA" | grep [^_]poll | head -6
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)
    "poll_count": *, (glob)
    "poll_time_us": *, (glob)

# Verify the packfile item counts are recorded in scuba
  $ jq -S .int "$SCUBA" | grep "packfile_"
    "packfile_commit_count": 2,
    "packfile_tag_count": 2,
    "packfile_tree_and_blob_count": 4,

# Verify the signature hashing haves and wants from the client are recorded in scuba
  $ jq -S .normal "$SCUBA" | grep "signature"
    "request_signature": "647444788fee66b8e43d7d5972a241adfc57b1cf",

# Verify the method variants in scuba as a normvector
  $ jq .normvector.method_variants "$SCUBA" | grep -v null 
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]
  [
    "standard"
  ]

# Verify the timed futures logged with log tags and msgs show up in scuba logs
  $ jq .normal "$SCUBA" | grep -e "Converted" -e "Counted" -e "Generated" -e "Collected" -e "Read" | sort
    "log_tag": "Collected Bonsai commits to send to client",
    "log_tag": "Converted HAVE Git commits to Bonsais",
    "log_tag": "Converted WANT Git commits to Bonsais",
    "log_tag": "Counted number of objects to be sent in packfile",
    "log_tag": "Generated commits stream",
    "log_tag": "Generated tags stream",
    "log_tag": "Generated trees and blobs stream",
    "msg": "Read",
    "msg": "Read",
    "msg": "Read",
    "msg": "Read",
    "msg": "Read",
    "msg": "Read",
    "msg": "Read",
