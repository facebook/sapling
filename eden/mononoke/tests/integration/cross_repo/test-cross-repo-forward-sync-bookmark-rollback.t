# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Enable bookmark polling before starting the forward syncer command.

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

The global cursor is the common starting point for bookmark mode and must stay
frozen while a per-bookmark cursor processes a new bookmark update.

  $ GLOBAL_COUNTER_BEFORE=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_from_1'")
  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_C
  > # exists: S_B $S_B
  > # message: S_C "bookmark mode update"
  > # modify: S_C feature_file "bookmark mode"
  > # bookmark: S_C feature
  > EOF

  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep "processing log entry"
  [INFO] processing log entry #* (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value = $GLOBAL_COUNTER_BEFORE FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_from_1'"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT COUNT(*) FROM mutable_counters WHERE repo_id = 0 AND name GLOB 'xreposync_by_bookmark_v1_1_*_feature'"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT COUNT(*) FROM mutable_counters WHERE repo_id = 0 AND name GLOB 'xreposync_by_bookmark_v1_initialized_*'"
  0

Disable bookmark polling and add another update to the same bookmark. The
legacy loop must skip the entry already covered by the per-bookmark cursor,
process only the new entry, and catch the global cursor up to the log head.

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:forward_syncer_bookmark_polling": false
  >   }
  > }
  > EOF

  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_C-S_D
  > # exists: S_C $S_C
  > # message: S_D "legacy mode update"
  > # modify: S_D feature_file "legacy mode"
  > # bookmark: S_D feature
  > EOF

  $ mononoke_x_repo_sync 1 0 tail --catch-up-once > "$TESTTMP/legacy-sync.out" 2>&1
  $ grep -c "processing log entry" "$TESTTMP/legacy-sync.out"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value = (SELECT MAX(id) FROM bookmarks_update_log WHERE repo_id = 1) FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_from_1'"
  1

The target bookmark reflects the update processed after rollback.

  $ TARGET_BONSAI=$(mononoke_admin bookmarks -R large-mon get bookprefix/feature)
  $ mononoke_admin changelog -R large-mon graph -i "$TARGET_BONSAI" -M | sed -n '1p'
  o  message: legacy mode update
