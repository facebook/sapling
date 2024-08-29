# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
  $ export LARGE_REPO_NAME="large-mon"
  $ export SMALL_REPO_NAME="small-mon"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark

  $ create_large_small_repo
  Adding synced mapping entry
  $ setup_configerator_configs
  $ enable_pushredirect 1
  $ start_large_small_repo
  Starting Mononoke server
  $ init_local_large_small_clones

-- Empty commit sync
  $ cd "$TESTTMP"/large-hg-client
  $ quiet hg up master_bookmark
  $ hg commit --config ui.allowemptycommit=True -m "Empty1"
  $ quiet hg push -r . --to master_bookmark

  $ quiet_grep "syncing bookmark" -- backsync_large_to_small
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

  $ cd "$TESTTMP/small-hg-client"
  $ quiet hg pull
  $ log -r master_bookmark
  o  Empty1 [public;rev=2;bcf0910445fc] default/master_bookmark
  │
  ~

-- Skip empty commits option
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:cross_repo_skip_backsyncing_ordinary_empty_commits": true
  >   }
  > }
  > EOF

  $ cd "$TESTTMP"/large-hg-client
  $ quiet hg up master_bookmark
  $ hg commit --config ui.allowemptycommit=True -m "Empty2"
  $ echo bar > smallrepofolder/baz
  $ hg add smallrepofolder/baz
  $ hg commit -m "non-empty after empty2"
  $ quiet hg push -r . --to master_bookmark

  $ quiet_grep "syncing bookmark" -- backsync_large_to_small
  * syncing bookmark master_bookmark to * (glob)
  $ flush_mononoke_bookmarks

  $ cd "$TESTTMP/small-hg-client"
  $ quiet hg pull
  $ log -r master_bookmark^::master_bookmark
  o  non-empty after empty2 [public;rev=3;*] default/master_bookmark (glob)
  │
  o  Empty1 [public;rev=2;*] (glob)
  │
  ~

Pushrebase of empty commit via small repo errors out as the commit rewrites into
nothingness. (But it succeeds in the end.)
  $ hg commit --config ui.allowemptycommit=True -m "Empty3"
  $ hg push -r . --to master_bookmark -q
  abort: failed reading from pipe: The read operation timed out
  [255]
  $ log -r master_bookmark -r .
  @  Empty3 [draft;rev=4;*] (glob)
  │
  ~
  $
  o  non-empty after empty2 [public;rev=3;919e7f2c67b8] default/master_bookmark
  │
  ~
Non-empty commit can go in via pushrebase
  $ echo 3 > file_3
  $ hg add file_3
  $ hg commit --amend -m "Non-empty-4"

  $ quiet hg push -r . --to master_bookmark
XXX (not sure why we don't end up on master just after push)
  $ quiet hg up master_bookmark
  $ log -r master_bookmark^::master_bookmark
  @  Non-empty-4 [public;rev=6;*] default/master_bookmark (glob)
  │
  o  non-empty after empty2 [public;rev=3;*] (glob)
  │
  ~

The large repo accepted all those pushes
  $ cd "$TESTTMP"/large-hg-client
  $ hg pull -q
  $ log -r master_bookmark^::master_bookmark
  o  Non-empty-4 [public;rev=7;*] default/master_bookmark (glob)
  │
  o  Empty3 [public;rev=6;*] (glob)
  │
  ~

Ensure that forward sync is not affcted by this tunable and empty commits still
make it to the large repo.
  $ killandwait $MONONOKE_PID
  $ mononoke
  $ wait_for_mononoke
  $ reset_pushredirect && enable_pushredirect 0 false false

  $ cd "$TESTTMP/small-hg-client"
  $ hg commit --config ui.allowemptycommit=True -m "Empty5" || exit 1
This time pushing empty commit shouldn't fail as there is no pushredirection.
  $ quiet hg push -r . --to master_bookmark

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 4)";
  $ quiet_grep processing -- mononoke_x_repo_sync 1 0 tail --catch-up-once
  * processing log entry #5 (glob)

  $ cd "$TESTTMP"/large-hg-client
  $ quiet hg pull
  $ log -r master_bookmark^::master_bookmark
  o  Empty5 [public;rev=8;*] default/master_bookmark (glob)
  │
  o  Non-empty-4 [public;rev=7;*] (glob)
  │
  ~
