# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

We use multiplex blobstore here as this one provides logging that we test later.
  $ export MULTIPLEXED=1

-- Enable logging of bookmark updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY

-- Init Mononoke thingies
  $ PUSHREBASE_REWRITE_DATES=1 create_large_small_repo
  Adding synced mapping entry
  $ setup_configerator_configs
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- Start up the backsyncer in the background
  $ backsync_large_to_small_forever

Before config change
-- push to a large repo
  $ cd "$TESTTMP"/large-hg-client
  $ hg up -q master_bookmark

  $ mkdir -p smallrepofolder
  $ echo bla > smallrepofolder/bla
  $ hg ci -Aqm "before config change"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ hg push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  before config change [public;*] remote/master_bookmark (glob)
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ hg pull -q
  $ hg up -q master_bookmark
  $ log -r master_bookmark
  @  before config change [public;rev=2;*] remote/master_bookmark (glob)
  │
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  bla

Config change
  $ update_commit_sync_map_first_option
-- let LiveCommitSyncConfig pick up the changes
  $ force_update_configerator

  $ cd "$TESTTMP"/large-hg-client
  $ hg up master_bookmark -q
  $ echo 1 >> 1 && hg add 1 && hg ci -m 'change of mapping'
  $ hg revert -r .^ 1
  $ hg commit --amend
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ hg push -r . --to master_bookmark -q

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"
  $ LARGE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id $REPOIDLARGE get master_bookmark)
  $ SMALL_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id $REPOIDSMALL get master_bookmark)
  $ update_mapping_version "$REPOIDSMALL" "$SMALL_MASTER_BONSAI" "$REPOIDLARGE" "$LARGE_MASTER_BONSAI" "new_version"

-- restart the backsyncer to empty the synced commit mapping cache
  $ killandwait $BACKSYNCER_PID
  $ backsync_large_to_small_forever

-- push to a large repo, using new path mapping
  $ cd "$TESTTMP"/large-hg-client
  $ hg up -q master_bookmark

  $ mkdir -p smallrepofolder_after
  $ echo baz > smallrepofolder_after/baz
  $ hg ci -Aqm "after config change"
  $ PREV_BOOK_VALUE=$(get_bookmark_value_edenapi small-mon master_bookmark)
  $ hg push -r . --to master_bookmark -q
  $ log -r master_bookmark
  o  after config change [public;rev=*;*] remote/master_bookmark (glob)
  │
  ~

-- wait a second to give backsyncer some time to catch up
  $ wait_for_bookmark_move_away_edenapi small-mon master_bookmark  "$PREV_BOOK_VALUE"

-- check the same commit in the small repo
  $ cd "$TESTTMP/small-hg-client"
  $ hg pull -q
  $ hg up -q master_bookmark
  $ log -r master_bookmark
  @  after config change [public;rev=*;*] remote/master_bookmark (glob)
  │
  ~
  $ hg log -r master_bookmark -T "{files % '{file}\n'}"
  baz

-- Check logging
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq --compact-output '[.repo_name, .changeset_id, .bookmark, .is_public]' | sort
  ["large-mon","*","master_bookmark",true] (glob)
  ["large-mon","*","master_bookmark",true] (glob)
  ["large-mon","*","master_bookmark",true] (glob)
  ["small-mon","*","master_bookmark",true] (glob)
  ["small-mon","*","master_bookmark",true] (glob)
  ["small-mon","*","master_bookmark",true] (glob)

  $ cat "$TESTTMP/scuba_backsyncer.json" | summarize_scuba_json "Backsyncing" \
  >     .normal.log_tag .int.backsync_duration_ms \
  >     .normal.source_repo_name .normal.target_repo_name \
  >     .normal.from_csid .normal.to_csid \
  >     .normal.backsync_previously_done \
  >     .int.backsyncer_bookmark_log_entry_id \
  >     .int.BlobGets \
  >     .int.SqlReadsMaster \
  >     .int.poll_count
  {
    \"BlobGets\": [1-9]\d*, (re)
    \"SqlReadsMaster\": [1-9]\d*, (re)
    \"backsync_duration_ms\": [1-9]\d*, (re)
    "backsync_previously_done": "false",
    "backsyncer_bookmark_log_entry_id": 2,
    "from_csid": "*", (glob)
    "log_tag": "Backsyncing",
    \"poll_count\": [1-9]\d*, (re)
    "source_repo_name": "large-mon",
    "target_repo_name": "small-mon",
    "to_csid": "*" (glob)
  }
  {
    \"BlobGets\": [1-9]\d*, (re)
    \"SqlReadsMaster\": [1-9]\d*, (re)
    \"backsync_duration_ms\": [1-9]\d*, (re)
    "backsync_previously_done": "false",
    "backsyncer_bookmark_log_entry_id": 3,
    "from_csid": "*", (glob)
    "log_tag": "Backsyncing",
    \"poll_count\": [1-9]\d*, (re)
    "source_repo_name": "large-mon",
    "target_repo_name": "small-mon",
    "to_csid": "*" (glob)
  }
  {
    \"BlobGets\": [1-9]\d*, (re)
    \"SqlReadsMaster\": [1-9]\d*, (re)
    \"backsync_duration_ms\": [1-9]\d*, (re)
    "backsync_previously_done": "false",
    "backsyncer_bookmark_log_entry_id": 4,
    "from_csid": "*", (glob)
    "log_tag": "Backsyncing",
    \"poll_count\": [1-9]\d*, (re)
    "source_repo_name": "large-mon",
    "target_repo_name": "small-mon",
    "to_csid": "*" (glob)
  }

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | sort | jq '{repo_name,bookmark_name,operation}'
  {
    "repo_name": "large-mon",
    "bookmark_name": "master_bookmark",
    "operation": "pushrebase"
  }
  {
    "repo_name": "large-mon",
    "bookmark_name": "master_bookmark",
    "operation": "pushrebase"
  }
  {
    "repo_name": "large-mon",
    "bookmark_name": "master_bookmark",
    "operation": "pushrebase"
  }
  {
    "repo_name": "small-mon",
    "bookmark_name": "master_bookmark",
    "operation": "update"
  }
  {
    "repo_name": "small-mon",
    "bookmark_name": "master_bookmark",
    "operation": "update"
  }
  {
    "repo_name": "small-mon",
    "bookmark_name": "master_bookmark",
    "operation": "update"
  }
