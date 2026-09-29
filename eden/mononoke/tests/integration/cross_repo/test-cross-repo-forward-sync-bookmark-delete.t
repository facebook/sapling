# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:forward_syncer_bookmark_polling": true
  >   }
  > }
  > EOF

  $ XREPOSYNC=1 init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

Create and forward-sync a publishing bookmark.

  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_C
  > # exists: S_B $S_B
  > # message: S_C "bookmark to delete"
  > # modify: S_C feature_file "feature"
  > # bookmark: S_C feature
  > EOF
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep "processing log entry"
  [INFO] processing log entry #* (glob)
  $ mononoke_admin bookmarks -R large-mon get bookprefix/feature > /dev/null

Delete the source bookmark. The next iteration discovers the target-only
bookmark by comparing the current source and target bookmark sets, then reads
the existing per-bookmark update-log stream to apply the deletion.

  $ quiet mononoke_admin bookmarks -R small-mon delete feature
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep "processing log entry"
  [INFO] processing log entry #* (glob)
  $ mononoke_admin bookmarks -R large-mon list | grep -c 'bookprefix/feature' || true
  0

The per-bookmark cursor covers the delete, and no global deletion cursor is
created.

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value = (SELECT MAX(id) FROM bookmarks_update_log WHERE repo_id = 1 AND CAST(name AS TEXT) = 'feature') FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_by_bookmark_v1_1_branch_feature'"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT COUNT(*) FROM mutable_counters WHERE repo_id = 0 AND name GLOB 'xreposync_deleted_bookmarks_v1_*'"
  0
