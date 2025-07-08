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

  $ mononoke_x_repo_sync 2 0 tail --bookmark-regex "master_bookmark" \
  > --catch-up-once |& grep -E '(processing|skipping)'
  [INFO] skipping log entry #2 for anotherbookmark
  [INFO] skipping log entry #3 for somebookmark
  [INFO] processing log entry #4
  [INFO] skipping log entry #5 for somebookmark
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5


-- use verify-bookmarks command. One inconsistency.
  $ crossrepo_verify_bookmarks 2 0
  [WARN] inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  [WARN] inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  Error: found 2 inconsistencies
  [1]

-- use verify-bookmarks, but passing a regex.
  $ crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates --bookmark-regex "master_bookmark"
  [INFO] all is well!


-- updating large repo bookmark will not work, bc there are unsynced commits.
  $ crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates
  [WARN] found 2 inconsistencies, trying to update them...
  Error: Missing outcome for * from small repo (glob)
  [1]

-- sync the missing commits
  $ mononoke_x_repo_sync 2 0 once --bookmark-regex ".+bookmark"
  [INFO] Starting session with id * (glob)
  [INFO] Starting up X Repo Sync from small repo ovr-mon to large repo meg-mon
  [INFO] Syncing 3 commits and all of their unsynced ancestors
  [INFO] Checking if 9f68d735e272dce827b1c41311f8e99a8ae9f10ed971f541f0ba1a76e606f832 is already synced 2->0
  [INFO] 9f68d735e272dce827b1c41311f8e99a8ae9f10ed971f541f0ba1a76e606f832 is already synced
  [INFO] Checking if 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f is already synced 2->0
  [INFO] 1 unsynced ancestors of 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f
  [INFO] syncing 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f
  [INFO] changeset 1bb2a7206ca6be0c58d221310122be17839ac6969017d940aa6ef6ca8eec495f synced as 8213e7f8c5768f72236f6d18cf84dfe5f6af4266c13da41d7eae97873d46e593 in * (glob)
  [INFO] successful sync
  [INFO] Checking if 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8 is already synced 2->0
  [INFO] 2 unsynced ancestors of 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8
  [INFO] syncing 814d6ccdf14dbc46142c13c098b59d316c98ee4dfd921f85a5d2186048142b24
  [INFO] changeset 814d6ccdf14dbc46142c13c098b59d316c98ee4dfd921f85a5d2186048142b24 synced as aa1d76f7d25dc8a93190a32de9c5784c3d2b57e0d0a3d92a52d98aca800f48b8 in * (glob)
  [INFO] syncing 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8
  [INFO] changeset 545278b8c8976a9d986b1ef0270e80cbf79ae8a7991af12fa437d19341d884a8 synced as bf8d1698e43e07e19660eca448c1c155aae5673a3c8f81cc53880ffda469fe6d in * (glob)
  [INFO] successful sync
  [INFO] X Repo Sync execution finished from small repo ovr-mon to large repo meg-mon

-- now update the inconsistent bookmarks
  $ crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks
  [WARN] found 2 inconsistencies, trying to update them...
  [INFO] setting ovrsource/* (glob)
  [INFO] setting ovrsource/* (glob)

-- mutable counter was not updated, because the once command was used
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5
