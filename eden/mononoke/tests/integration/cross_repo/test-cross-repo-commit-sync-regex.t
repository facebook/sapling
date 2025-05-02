# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ init_two_small_one_large_repo
  A=e258521a78f8e12bee03bda35489701d887c41fd
  A=8ca76aa82bf928df58db99489fa17938e39774e4
  A=6ebc043d84761f4b77f73e4a2034cf5669bb6a54

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 2 get master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- start mononoke
  $ start_and_wait_for_mononoke_server
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 0)";
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep -E '(processing|skipping)'
  * processing log entry * (glob)

-- setup hg client repos
  $ cd "$TESTTMP"
  $ setconfig remotenames.selectivepulldefault=master_bookmark,somebookmark
  $ hg clone -q mono:fbs-mon fbs-hg-cnt --noupdate
  $ hg clone -q mono:ovr-mon ovr-hg-cnt --noupdate
  $ hg clone -q mono:meg-mon meg-hg-cnt --noupdate

-- push to a bookmark that won't be synced
  $ cd "$TESTTMP"/ovr-hg-cnt
  $ hg up -q master_bookmark
  $ createfile arvr/branchfile
  $ hg -q ci -m "ovrsource branch commit"
  $ hg push -r . --to anotherbookmark -q --create
  $ hg up -q .^
  $ createfile arvr/somefile
  $ hg -q ci -m "ovrsource commit 2"
  $ hg push -r . --to somebookmark -q --create

-- now push to master
  $ hg up -q master_bookmark
  $ createfile arvr/newfile
  $ hg -q ci -m "ovrsource commit 3"
  $ hg push -r . --to master_bookmark -q
  $ hg up somebookmark -q
  $ createfile arvr/somefile2
  $ hg -q ci -m "ovrsource commit 4"
  $ hg push -r . --to somebookmark -q --create

  $ with_stripped_logs mononoke_x_repo_sync 2 0 tail --bookmark-regex "master_bookmark" \
  > --catch-up-once |& grep -E '(processing|skipping)'
  skipping log entry #2 for anotherbookmark
  skipping log entry #3 for somebookmark
  processing log entry #4
  skipping log entry #5 for somebookmark
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5


-- use verify-bookmarks command. One inconsistency.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0
  inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  Error: found 2 inconsistencies
  [1]

-- use verify-bookmarks, but passing a regex.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates --bookmark-regex "master_bookmark"
  all is well!


-- updating large repo bookmark will not work, bc there are unsynced commits.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates
  found 2 inconsistencies, trying to update them...
  Error: Missing outcome for * from small repo (glob)
  [1]

-- sync the missing commits
  $ with_stripped_logs mononoke_x_repo_sync 2 0 once --bookmark-regex ".+bookmark"
  Starting session with id * (glob)
  Starting up X Repo Sync from small repo ovr-mon to large repo meg-mon
  Syncing 3 commits and all of their unsynced ancestors
  Checking if 9f68d735e272dce827b1c41311f8e99a8ae9f10ed971f541f0ba1a76e606f832 is already synced 2->0
  9f68d735e272dce827b1c41311f8e99a8ae9f10ed971f541f0ba1a76e606f832 is already synced
  Checking if 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f is already synced 2->0
  1 unsynced ancestors of 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f
  syncing 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f
  changeset 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f synced as 8213e7f8c5768f72236f6d18cf84dfe5f6af4266c13da41d7eae97873d46e593 in * (glob)
  successful sync
  Checking if 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8 is already synced 2->0
  2 unsynced ancestors of 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8
  syncing 814d6ccdf14dbc46142c13c098b59d316c98ee4dfd921f85a5d2186048142b24
  changeset 814d6ccdf14dbc46142c13c098b59d316c98ee4dfd921f85a5d2186048142b24 synced as aa1d76f7d25dc8a93190a32de9c5784c3d2b57e0d0a3d92a52d98aca800f48b8 in * (glob)
  syncing 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8
  changeset 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8 synced as bf8d1698e43e07e19660eca448c1c155aae5673a3c8f81cc53880ffda469fe6d in * (glob)
  successful sync
  X Repo Sync execution finished from small repo ovr-mon to large repo meg-mon

-- now update the inconsistent bookmarks
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks
  found 2 inconsistencies, trying to update them...
  setting ovrsource/* (glob)
  setting ovrsource/* (glob)

-- mutable counter was not updated, because the once command was used
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5
