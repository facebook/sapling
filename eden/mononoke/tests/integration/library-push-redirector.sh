#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"

function verify_wc() {
   local large_repo_commit
   large_repo_commit="$1"
   "$MONONOKE_ADMIN" "${COMMON_ARGS[@]}" --log-level ERROR \
     --mononoke-config-path  "$TESTTMP"/mononoke-config \
     --source-repo-id="$REPOIDLARGE" --target-repo-id="$REPOIDSMALL" \
     crossrepo verify-wc "$large_repo_commit"
}

function validate_commit_sync() {
  local entry_id
  entry_id="$1"
  shift
  "$COMMIT_VALIDATOR" "${COMMON_ARGS[@]}" --debug --repo-id "$REPOIDLARGE" \
   --mononoke-config-path "$TESTTMP/mononoke-config" \
   --test-instance \
   --local-configerator-path="$TESTTMP/configerator" \
   --master-bookmark=master_bookmark \
   once --entry-id "$entry_id" "$@"
}

function create_large_small_repo() {
  REPOTYPE="blob_files"
  ENABLE_PRESERVE_BUNDLE2=1 REPOID=0 REPONAME=large-mon setup_common_config "$REPOTYPE"
  ENABLE_PRESERVE_BUNDLE2=1 REPOID=1 REPONAME=small-mon setup_common_config "$REPOTYPE"
cat >> "$TESTTMP/mononoke-config/common/commitsyncmap.toml" <<EOF
[megarepo_test]
large_repo_id = 0
common_pushrebase_bookmarks = ["master_bookmark"]
 [[megarepo_test.small_repos]]
 repoid = 1
 bookmark_prefix = "bookprefix/"
 default_action = "prepend_prefix"
 default_prefix = "smallrepofolder"
 direction = "large_to_small"
    [megarepo_test.small_repos.mapping]
    "non_path_shifting" = "non_path_shifting"
EOF

  if [ -n "$COMMIT_SYNC_CONF" ]; then
    cat > "$COMMIT_SYNC_CONF/current" << EOF
{
  "repos": {
    "megarepo_test": {
      "large_repo_id": 0,
      "common_pushrebase_bookmarks": ["master_bookmark"],
      "small_repos": [
        {
          "repoid": 1,
          "bookmark_prefix": "bookprefix/",
          "default_action": "prepend_prefix",
          "default_prefix": "smallrepofolder",
          "direction": "large_to_small",
          "mapping": {
            "non_path_shifting": "non_path_shifting"
          }
        }
      ]
    }
  }
}
EOF

  fi

  echo "Setting up hg server repos"

  cd "$TESTTMP" || exit 1
  hginit_treemanifest small-hg-srv
  cd small-hg-srv || exit 1
  echo 1 > file.txt
  hg addremove -q && hg ci -q -m 'pre-move commit'

  cd ..
  cp -r small-hg-srv large-hg-srv
  cd large-hg-srv || exit 1
  mkdir smallrepofolder
  hg mv file.txt smallrepofolder/file.txt
  hg ci -m 'move commit'
  create_first_post_move_commit smallrepofolder
  hg book -r . master_bookmark
  cat >> .hg/hgrc <<EOF
[extensions]
pushrebase =
EOF

  cd ..
  cd small-hg-srv || exit 1
  cat >> .hg/hgrc <<EOF
[extensions]
pushrebase =
EOF
  create_first_post_move_commit .
  hg book -r . master_bookmark

  echo "Blobimporting them"
  cd "$TESTTMP" || exit 1
  export REPOIDLARGE=0
  export REPOIDSMALL=1
  REPOID="$REPOIDLARGE" blobimport large-hg-srv/.hg large-mon
  REPOID="$REPOIDSMALL" blobimport small-hg-srv/.hg small-mon

  init_client small-hg-srv small-hg-client
  cd "$TESTTMP" || exit 1
  init_client large-hg-srv large-hg-client

  export LARGE_MASTER_BONSAI
  LARGE_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDLARGE master_bookmark)
  export SMALL_MASTER_BONSAI
  SMALL_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDSMALL master_bookmark)

  echo "Adding synced mapping entry"
  add_synced_commit_mapping_entry "$REPOIDSMALL" "$SMALL_MASTER_BONSAI" \
   "$REPOIDLARGE" "$LARGE_MASTER_BONSAI"
}

function init_large_small_repo() {
  create_large_small_repo
  start_large_small_repo "$@"
}

function start_large_small_repo {
  echo "Starting Mononoke server"
  mononoke "$@"
  wait_for_mononoke
  # Setting XREPOSYNC allows the user of this fn to set x-repo sync mutable counter instead of the backsync one
  # This is useful, if the intention is to use x-repo sync instead of backsync after the setup
  if [[ -n "${XREPOSYNC:-}" ]]; then
    sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDLARGE, 'xreposync_from_$REPOIDSMALL', 2)";
  else
    sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDSMALL, 'backsync_from_$REPOIDLARGE', 2)";
  fi
}

function createfile {
  mkdir -p "$(dirname "$1")" && echo "$1" > "$1" && hg add -q "$1";
}

function create_first_post_move_commit {
 echo 1 > "$1/filetoremove" && hg add "$1/filetoremove" && hg ci -m 'first post-move commit'
 hg revert -r .^ "$1/filetoremove"
}

function init_client() {
  cd "$TESTTMP" || exit 1
  hgclone_treemanifest ssh://user@dummy/"$1" "$2" --noupdate --config extensions.remotenames=
  cd "$TESTTMP/$2" || exit 1
  cat >> .hg/hgrc <<EOF
[extensions]
pushrebase =
remotenames =
EOF
}

function backsync_large_to_small() {
  "$BACKSYNCER" "${COMMON_ARGS[@]}" --debug --source-repo-id "$REPOIDLARGE" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --target-repo-id "$REPOIDSMALL" \
    backsync-all
}

function backsync_large_to_small_forever {
  "$BACKSYNCER" "${COMMON_ARGS[@]}" --debug \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --source-repo-id "$REPOIDLARGE" \
    --target-repo-id "$REPOIDSMALL" \
    --test-instance \
    "$@" \
    backsync-forever >> "$TESTTMP/backsyncer.out" 2>&1 &

  export BACKSYNCER_PID=$!
  echo "$BACKSYNCER_PID" >> "$DAEMON_PIDS"
}

function mononoke_x_repo_sync_forever() {
  source_repo_id=$1
  target_repo_id=$2
  shift
  shift
  GLOG_minloglevel=5 "$MONONOKE_X_REPO_SYNC" \
    "${COMMON_ARGS[@]}" \
    --test-instance \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --source-repo-id "$source_repo_id" \
    --target-repo-id "$target_repo_id" \
    "$@" \
    tail --sleep-secs=1 >> "$TESTTMP/xreposync.out" 2>&1 &

  export XREPOSYNC_PID=$!
  echo "$XREPOSYNC_PID" >> "$DAEMON_PIDS"
}
