#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
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
   --master-bookmark=master_bookmark \
   once --entry-id "$entry_id" "$@"
}

function large_small_config() {
  REPOTYPE="blob_files"
  REPOID=0 REPONAME=large-mon setup_common_config "$REPOTYPE"
  REPOID=1 REPONAME=small-mon setup_common_config "$REPOTYPE"
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
      ],
      "version_name": "test_version"
    }
  }
}
EOF

    cat > "$COMMIT_SYNC_CONF/all" << EOF
{
  "repos": {
    "megarepo_test": {
      "versions": [
        {
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
          ],
          "version_name": "test_version"
        }
      ],
      "common": {
        "common_pushrebase_bookmarks": ["master_bookmark"],
        "large_repo_id": 0,
        "small_repos": {
          1: {
            "bookmark_prefix": "bookprefix/"
          }
        }
      }
    }
  }
}
EOF

  fi

}

function large_small_setup() {
  quiet testtool_drawdag -R small-mon <<'EOF'
S_B
|
S_A
# message: S_A "pre-move commit"
# author: S_A test
# modify: S_A file.txt "1\n"
# forget: S_A S_A
# message: S_B "first post-move commit"
# author: S_B test
# modify: S_B filetoremove "1\n"
# bookmark: S_B master_bookmark
# forget: S_B S_B
EOF

  export SMALL_MASTER_BONSAI
  SMALL_MASTER_BONSAI=$S_B

  quiet testtool_drawdag -R large-mon <<'EOF'
L_C
|
L_B
|
L_A
# message: L_A "pre-move commit"
# author: L_A test
# modify: L_A file.txt "1\n"
# message: L_B "move commit"
# author: L_B test
# forget: L_A L_A
# copy: L_B smallrepofolder/file.txt "1\n" L_A file.txt
# delete: L_B file.txt
# forget: L_B L_B
# message: L_C "first post-move commit"
# author: L_C test
# modify: L_C smallrepofolder/filetoremove "1\n"
# bookmark: L_C master_bookmark
# forget: L_C L_C
EOF

  export LARGE_MASTER_BONSAI
  LARGE_MASTER_BONSAI=$L_C

  echo "Adding synced mapping entry"
  export REPOIDLARGE=0
  export REPOIDSMALL=1
  add_synced_commit_mapping_entry "$REPOIDSMALL" "$SMALL_MASTER_BONSAI" \
   "$REPOIDLARGE" "$LARGE_MASTER_BONSAI" "test_version"

}

function create_large_small_repo() {
  large_small_config
  large_small_setup
}

function update_mapping_version {
  local small_repo_id large_repo_id small_bcs_id large_bcs_id version
  small_repo_id="$1"
  small_bcs_id="$2"
  large_repo_id="$3"
  large_bcs_id="$4"
  version="$5"
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" <<EOF
    REPLACE INTO synced_commit_mapping (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
    VALUES ('$small_repo_id', X'$small_bcs_id', '$large_repo_id', X'$large_bcs_id', '$version');
EOF

}

function update_commit_sync_map_first_option {
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
          "default_prefix": "smallrepofolder_after",
          "direction": "large_to_small",
          "mapping": {
            "non_path_shifting": "non_path_shifting"
          }
        }
      ],
      "version_name": "new_version"
    }
  }
}
EOF

  cat > "$COMMIT_SYNC_CONF/all" << EOF
{
  "repos": {
    "megarepo_test": {
      "versions": [
        {
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
          ],
          "version_name": "test_version"
        },
      {
        "large_repo_id": 0,
        "common_pushrebase_bookmarks": ["master_bookmark"],
        "small_repos": [
          {
            "repoid": 1,
            "bookmark_prefix": "bookprefix/",
            "default_action": "prepend_prefix",
            "default_prefix": "smallrepofolder_after",
            "direction": "large_to_small",
            "mapping": {
              "non_path_shifting": "non_path_shifting"
            }
          }
        ],
        "version_name": "new_version"
      }
      ],
      "common": {
        "common_pushrebase_bookmarks": ["master_bookmark"],
        "large_repo_id": 0,
        "small_repos": {
          1: {
            "bookmark_prefix": "bookprefix/"
          }
        }
      }
    }
  }
}
EOF

}

function update_commit_sync_map_second_option {
  cat > "$COMMIT_SYNC_CONF/current" <<EOF
{
  "repos": {
    "megarepo_test": {
        "large_repo_id": 0,
        "common_pushrebase_bookmarks": [
          "master_bookmark"
        ],
        "small_repos": [
          {
            "repoid": 1,
            "default_action": "prepend_prefix",
            "default_prefix": "smallrepofolder1",
            "bookmark_prefix": "bookprefix1/",
            "mapping": {
              "special": "specialsmallrepofolder_after_change"
            },
            "direction": "large_to_small"
          }
        ],
        "version_name": "TEST_VERSION_NAME_LIVE_V2"
    }
  }
}
EOF

  cat > "$COMMIT_SYNC_CONF/all" << EOF
{
  "repos": {
    "megarepo_test": {
      "versions": [
        {
          "large_repo_id": 0,
          "common_pushrebase_bookmarks": ["master_bookmark"],
          "small_repos": [
            {
              "repoid": 1,
              "default_action": "prepend_prefix",
              "default_prefix": "smallrepofolder1",
              "bookmark_prefix": "bookprefix1/",
              "mapping": {
                "special": "specialsmallrepofolder1"
              },
              "direction": "large_to_small"
            }
          ],
          "version_name": "TEST_VERSION_NAME_LIVE_V1"
        },
        {
          "large_repo_id": 0,
          "common_pushrebase_bookmarks": [
            "master_bookmark"
          ],
          "small_repos": [
            {
              "repoid": 1,
              "default_action": "prepend_prefix",
              "default_prefix": "smallrepofolder1",
              "bookmark_prefix": "bookprefix1/",
              "mapping": {
                "special": "specialsmallrepofolder_after_change"
              },
              "direction": "large_to_small"
            }
          ],
          "version_name": "TEST_VERSION_NAME_LIVE_V2"
        }
      ],
      "common": {
        "common_pushrebase_bookmarks": ["master_bookmark"],
        "large_repo_id": 0,
        "small_repos": {
          1: {
            "bookmark_prefix": "bookprefix1/"
          }
        }
      }
    }
  }
}
EOF

}

function init_large_small_repo() {
  create_large_small_repo
  start_large_small_repo "$@"
  init_local_large_small_clones
}

function init_local_large_small_clones {
  cd "$TESTTMP" || exit 1
  REPONAME=small-mon hgmn_clone "mononoke://$(mononoke_address)/small-mon" small-hg-client --config extensions.remotenames=
  cat >> small-hg-client/.hg/hgrc <<EOF
[extensions]
pushrebase =
EOF
  REPONAME=large-mon hgmn_clone "mononoke://$(mononoke_address)/large-mon" large-hg-client --config extensions.remotenames=
  cat >> large-hg-client/.hg/hgrc <<EOF
[extensions]
pushrebase =
EOF
}

function start_large_small_repo {
  echo "Starting Mononoke server"
  mononoke "$@"
  wait_for_mononoke
  # Setting XREPOSYNC allows the user of this fn to set x-repo sync mutable counter instead of the backsync one
  # This is useful, if the intention is to use x-repo sync instead of backsync after the setup
  if [[ -v XREPOSYNC ]]; then
    sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDLARGE, 'xreposync_from_$REPOIDSMALL', 1)";
  else
    sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDSMALL, 'backsync_from_$REPOIDLARGE', 1)";
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
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --source-repo-id "$source_repo_id" \
    --target-repo-id "$target_repo_id" \
    "$@" \
    tail --sleep-secs=1 >> "$TESTTMP/xreposync.out" 2>&1 &

  export XREPOSYNC_PID=$!
  echo "$XREPOSYNC_PID" >> "$DAEMON_PIDS"
}

# Wait for xrepo sync (15s at most)
function wait_for_xrepo_sync {
  bookmark_update_log_entry="$1"
  local attempts=150
  for _ in $(seq 1 $attempts); do
    grep -q "successful sync bookmark update log #$bookmark_update_log_entry" "$TESTTMP/xreposync.out" && break
    sleep 0.1
  done

  if ! grep -q "successful sync bookmark update log #$bookmark_update_log_entry" "$TESTTMP/xreposync.out"; then
    echo "xrepo sync of bookmark update log entry $1 did not finish" >&2
    cat "$TESTTMP/xreposync.out"
    exit 1
  fi
}


function init_two_small_one_large_repo() {
  # setup configuration
  # Disable bookmarks cache because bookmarks are modified by two separate processes
  REPOTYPE="blob_files"
  REPOID=0 REPONAME=meg-mon setup_common_config $REPOTYPE
  REPOID=1 REPONAME=fbs-mon setup_common_config $REPOTYPE
  REPOID=2 REPONAME=ovr-mon setup_common_config $REPOTYPE

  cat >> "$HGRCPATH" <<EOF
[ui]
ssh="$DUMMYSSH"
[extensions]
amend=
pushrebase=
remotenames=
EOF

  setup_commitsyncmap
  setup_configerator_configs

  # setup hg server repos

  function createfile { mkdir -p "$(dirname  "$1")" && echo "$1" > "$1" && hg add -q "$1"; }
  function createfile_with_content { mkdir -p "$(dirname  "$1")" && echo "$2" > "$1" && hg add -q "$1"; }

  # init fbsource
  cd "$TESTTMP" || exit 1
  hginit_treemanifest fbs-hg-srv
  cd fbs-hg-srv || exit 1
  # create an initial commit, which will be the last_synced_commit
  createfile fbcode/fbcodefile_fbsource
  createfile arvr/arvrfile_fbsource
  createfile otherfile_fbsource
  hg -q ci -m "fbsource commit 1" && hg book -ir . master_bookmark

  # init ovrsource
  cd "$TESTTMP" || exit 1
  hginit_treemanifest ovr-hg-srv
  cd ovr-hg-srv || exit 1
  createfile fbcode/fbcodefile_ovrsource
  createfile arvr/arvrfile_ovrsource
  createfile otherfile_ovrsource
  createfile Research/researchfile_ovrsource
  hg -q ci -m "ovrsource commit 1" && hg book -r . master_bookmark

  # init megarepo - note that some paths are shifted, but content stays the same
  cd "$TESTTMP" || exit 1
  hginit_treemanifest meg-hg-srv
  cd meg-hg-srv || exit 1
  createfile fbcode/fbcodefile_fbsource
  createfile_with_content .fbsource-rest/arvr/arvrfile_fbsource arvr/arvrfile_fbsource
  createfile otherfile_fbsource
  createfile_with_content .ovrsource-rest/fbcode/fbcodefile_ovrsource fbcode/fbcodefile_ovrsource
  createfile arvr/arvrfile_ovrsource
  createfile_with_content arvr-legacy/otherfile_ovrsource otherfile_ovrsource
  createfile_with_content arvr-legacy/Research/researchfile_ovrsource Research/researchfile_ovrsource
  hg -q ci -m "megarepo commit 1"
  hg book -r . master_bookmark

  # blobimport hg servers repos into Mononoke repos
  cd "$TESTTMP"
  REPOID=0 blobimport meg-hg-srv/.hg meg-mon
  REPOID=1 blobimport fbs-hg-srv/.hg fbs-mon
  REPOID=2 blobimport ovr-hg-srv/.hg ovr-mon
}
