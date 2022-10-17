# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ init_two_small_one_large_repo

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 2 master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/ovr-hg-srv ovr-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate

-- start mononoke
  $ start_and_wait_for_mononoke_server
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 0)";
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep -E '(processing|skipping)'
  * processing log entry * (glob)

-- push to a bookmark that won't be synced
  $ cd "$TESTTMP"/ovr-hg-cnt
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ createfile arvr/somefile
  $ hg -q ci -m "ovrsource commit 2"
  $ REPONAME=ovr-mon hgmn push -r . --to somebookmark -q --create

-- now push to master
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ createfile arvr/newfile
  $ hg -q ci -m "ovrsource commit 3"
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark -q
  $ REPONAME=ovr-mon hgmn up somebookmark -q
  $ createfile arvr/somefile2
  $ hg -q ci -m "ovrsource commit 4"
  $ REPONAME=ovr-mon hgmn push -r . --to somebookmark -q --create

  $ mononoke_x_repo_sync 2 0 tail --bookmark-regex "master_bookmark" --catch-up-once |& grep -E '(processing|skipping)'
  * skipping log entry #2 for somebookmark (glob)
  * processing log entry * (glob)
  * skipping log entry #4 for somebookmark (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|4
