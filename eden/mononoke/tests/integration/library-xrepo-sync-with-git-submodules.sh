#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
. "${TEST_FIXTURES}/library.sh"

# Run initial setup (e.g. sync configs, small & large repos)
REPOTYPE="blob_files"
LARGE_REPO_NAME="large_repo"
LARGE_REPO_ID=0
SMALL_REPO_NAME="small_repo"
SMALL_REPO_ID=1

# Used by integration tests that source this file
# shellcheck disable=SC2034
NEW_BOOKMARK_NAME="SYNCED_HEAD"

LATEST_CONFIG_VERSION_NAME="INITIAL_IMPORT_SYNC_CONFIG"

ENABLE_API_WRITES=1 REPOID="$LARGE_REPO_ID" REPONAME="$LARGE_REPO_NAME" setup_common_config "$REPOTYPE"
REPOID="$SMALL_REPO_ID" REPONAME="$SMALL_REPO_NAME" setup_common_config "$REPOTYPE"

# By default, the `git_submodules_action` will be `STRIP`, meaning that any
# changes to git submodules will not be synced to the large repo.
function default_small_repo_config {
  jq . << EOF
  {
    "repoid": 1,
    "default_action": "prepend_prefix",
    "default_prefix": "smallrepofolder1",
    "bookmark_prefix": "bookprefix1/",
    "mapping": {
      "special": "specialsmallrepofolder_after_change"
    },
    "direction": "small_to_large"
  }
EOF
}

# Sets up a config to sync commits from a small repo to a large repo.
# By default, the `git_submodules_action` will be `STRIP`, meaning that any
# changes to git submodules will not be synced to the large repo.
function default_initial_import_config {
  SMALL_REPO_CFG=$(default_small_repo_config)
  jq . << EOF
  {
    "repos": {
      "large_repo": {
        "versions": [
          {
            "large_repo_id": $LARGE_REPO_ID,
            "common_pushrebase_bookmarks": [],
            "small_repos": [
              $SMALL_REPO_CFG
            ],
            "version_name": "$LATEST_CONFIG_VERSION_NAME"
          }
        ],
        "common": {
          "common_pushrebase_bookmarks": [],
          "large_repo_id": $LARGE_REPO_ID,
          "small_repos": {
            "$SMALL_REPO_ID": {
              "bookmark_prefix": "bookprefix1/"
            }
          }
        }
      }
    }
  }
EOF
}

# Modify a small repo config in a specific config version to keep the git
# submodules
function keep_git_submodules_in_config_version {
  VERSION_NAME=$1
  MOD_SMALL_REPO=$2

  TEMP_FILE="/tmp/COMMIT_SYNC_CONF_all"

  jq ".repos.large_repo.versions |= map(if .version_name != \"$VERSION_NAME\" then . else  .small_repos |= map(if .repoid == $MOD_SMALL_REPO then . + {\"git_submodules_action\": 1} else . end) end)" "$COMMIT_SYNC_CONF/all" > "$TEMP_FILE"

  cat "$TEMP_FILE" > "$COMMIT_SYNC_CONF/all"
}

function setup_sync_config_stripping_git_submodules {
  default_initial_import_config  > "$COMMIT_SYNC_CONF/all"
}

function run_common_xrepo_sync_with_gitsubmodules_setup {
  setup_sync_config_stripping_git_submodules

  start_and_wait_for_mononoke_server

  cd "$TESTTMP" || exit
}

function clone_and_log_large_repo {
  LARGE_BCS_IDS=( "$@" )
  cd "$TESTTMP" || exit
  REPONAME="$LARGE_REPO_NAME" hgmn_clone "mononoke://$(mononoke_address)/$LARGE_REPO_NAME" "$LARGE_REPO_NAME"
  cd "$LARGE_REPO_NAME" || exit


  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    LARGE_CS_ID=$(mononoke_newadmin convert --from bonsai --to hg -R "$LARGE_REPO_NAME" "$LARGE_BCS_ID" --derive)
    hg pull -q -r "$LARGE_CS_ID"
  done

  hg log --graph -T '{node|short} {desc}\n' --stat -r "all()"

  printf "\n\nRunning mononoke_admin to verify mapping\n\n"
  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    quiet_grep RewrittenAs -- with_stripped_logs mononoke_admin_source_target "$LARGE_REPO_ID" "$SMALL_REPO_ID" crossrepo map "$LARGE_BCS_ID"
  done

  printf "\nDeriving all the enabled derived data types\n"
  for LARGE_BCS_ID in "${LARGE_BCS_IDS[@]}"; do
    quiet mononoke_newadmin derived-data -R "$LARGE_REPO_NAME" derive --all-types -i "$LARGE_BCS_ID"
  done
}
