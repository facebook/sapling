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

Create two independent publishing bookmarks after the legacy global cursor was
initialized.

  $ GLOBAL_COUNTER_BEFORE=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_from_1'")
  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_C
  > # exists: S_B $S_B
  > # message: S_C "first bookmark update"
  > # modify: S_C first_file "first"
  > # bookmark: S_C feature-one
  > EOF
  $ quiet testtool_drawdag -R small-mon <<EOF
  > S_B-S_D
  > # exists: S_B $S_B
  > # message: S_D "second bookmark update"
  > # modify: S_D second_file "second"
  > # bookmark: S_D feature-two
  > EOF

Both bookmark streams are processed while the global cursor remains frozen.

  $ mononoke_x_repo_sync 1 0 tail --catch-up-once > "$TESTTMP/bookmark-sync.out" 2>&1
  $ grep -c "processing log entry" "$TESTTMP/bookmark-sync.out"
  2
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT value = $GLOBAL_COUNTER_BEFORE FROM mutable_counters WHERE repo_id = 0 AND name = 'xreposync_from_1'"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT COUNT(*) FROM mutable_counters WHERE repo_id = 0 AND name GLOB 'xreposync_by_bookmark_v1_1_*_feature-*'"
  2

The target bookmarks point to the independently synchronized commits.

  $ FIRST_TARGET=$(mononoke_admin bookmarks -R large-mon get bookprefix/feature-one)
  $ mononoke_admin changelog -R large-mon graph -i "$FIRST_TARGET" -M | sed -n '1p'
  o  message: first bookmark update
  $ SECOND_TARGET=$(mononoke_admin bookmarks -R large-mon get bookprefix/feature-two)
  $ mononoke_admin changelog -R large-mon graph -i "$SECOND_TARGET" -M | sed -n '1p'
  o  message: second bookmark update
