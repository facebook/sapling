  $ . $TESTDIR/library.sh

setup configuration

  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ cp "$TESTDIR/pushrebase_replay.bundle" "$TESTTMP/handle"
  $ create_pushrebaserecording_sqlite3_db
  $ init_pushrebaserecording_sqlite3_db
  $ cd $TESTTMP

setup a script to handle failures
  $ cat >> $TESTTMP/onfailure.sh <<EOF
  > echo "Failure handling."
  > EOF
  $ chmod +x $TESTTMP/onfailure.sh

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo foo > a
  $ echo foo > b
  $ hg addremove && hg ci -m 'initial'
  adding a
  adding b
  $ echo 'bar' > a
  $ hg addremove && hg ci -m 'a => bar'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Make client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames= -q

Push to Mononoke
  $ cd $TESTTMP/client-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hg up -q tip

  $ mkcommit pushcommit
  $ hgmn push -r . --to master_bookmark -q

  $ mkcommit anothercommit
  $ hgmn push -r . --to master_bookmark -q

  $ hgmn push -r .^ --to master_bookmark -q --non-forward-move --pushvar NON_FAST_FORWARD=true
  [1]

Check that new entry was added to the sync database. 3 pushes and 1 blobimport
  $ sqlite3 "$TESTTMP/repo/books" "select count(*) from bookmarks_update_log";
  4

Sync it to another client
  $ cd $TESTTMP
  $ cat >> repo-hg/.hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF


Make a copy of it that will be used later
  $ cp -r repo-hg repo-hg-2
  $ cp -r repo-hg repo-hg-3

Try to sync blobimport bookmark move, which should fail
  $ mononoke_hg_sync_with_failure_handler repo-hg 0 $TESTTMP/onfailure.sh
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #1 ... (glob)
  * running a failure handler: "$TESTTMP/onfailure.sh" (glob)
  Failure handling.
  * sync failed for #1 (glob)
  * caused by: unexpected bookmark move: blobimport (glob)

Sync a pushrebase bookmark move
  $ mononoke_hg_sync repo-hg 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #2 ... (glob)
  * successful sync (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  changeset:   2:1e43292ffbb3
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 2
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #3 ... (glob)
  * successful sync (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  changeset:   3:6cc06ef82eeb
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     anothercommit
  
  $ hg log -r master_bookmark^
  changeset:   2:1e43292ffbb3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 3
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #4 ... (glob)
  * successful sync (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  changeset:   2:1e43292ffbb3
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
Sync with incorrect timestamps, make sure replay fails
  $ cd $TESTTMP

  $ cat >>$TESTTMP/replayverification.py <<EOF
  > import os, sys
  > expected_book = os.environ["HG_EXPECTED_ONTOBOOK"]
  > expected_head = os.environ["HG_EXPECTED_REBASEDHEAD"]
  > actual_book = os.environ["HG_KEY"]
  > actual_head = os.environ["HG_NEW"]
  > if expected_book == actual_book and expected_head == actual_head:
  >     print "[ReplayVerification] Everything seems in order"
  >     sys.exit(0)
  > print "[ReplayVerification] Expected: (%s, %s). Actual: (%s, %s)" % (expected_book, expected_head, actual_book, actual_head)
  > sys.exit(1)
  > EOF

  $ cd repo-hg-2
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python "$TESTTMP/replayverification.py"
  > CONFIG
  $ hg log -r master_bookmark
  changeset:   1:add0c792bfce
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
  $ cd $TESTTMP
  $ sqlite3 "$TESTTMP/repo/books" "update bundle_replay_data set commit_hashes_json = '{\"1e43292ffbb38fa183e7f21fb8e8a8450e61c890\":10000000000}' where bookmark_update_log_id = 2"
  $ mononoke_hg_sync repo-hg-2 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #2 ... (glob)
  * sync failed for #2 (glob)
  * caused by: hg command failed: stdout: '', stderr: 'remote: pushing 1 changeset: (glob)
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] Expected: (master_bookmark, 1e43292ffbb38fa183e7f21fb8e8a8450e61c890). Actual: (master_bookmark, acc06228d802cbe9e2a6740c0abacf017f3be65c)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  error:pushkey
  '

Set the correct timestamp back
  $ sqlite3 "$TESTTMP/repo/books" "update bundle_replay_data set commit_hashes_json = '{\"1e43292ffbb38fa183e7f21fb8e8a8450e61c890\":0}' where bookmark_update_log_id = 2"

  $ cd repo-hg-2
  $ hg log -r master_bookmark
  changeset:   1:add0c792bfce
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
Replay in a loop
  $ cd $TESTTMP
  $ create_mutable_counters_sqlite3_db
  $ mononoke_hg_sync_loop repo-hg-3 0
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #1 ... (glob)
  * sync failed for #1 (glob)
  * caused by: unexpected bookmark move: blobimport (glob)
  $ mononoke_hg_sync_loop repo-hg-3 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #2 ... (glob)
  * successful sync (glob)
  * syncing log entry #3 ... (glob)
  * successful sync (glob)
  * syncing log entry #4 ... (glob)
  * successful sync (glob)
  $ sqlite3 "$TESTTMP/repo/mutable_counters" "select * from mutable_counters";
  0|latest-replayed-request|4

Make one more push from the client
  $ cd $TESTTMP/client-push
  $ hg up -q master_bookmark
  $ mkcommit onemorecommit
  $ hgmn push -r . --to master_bookmark -q

Continue replay
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #5 ... (glob)
  * successful sync (glob)
  $ cd $TESTTMP/repo-hg-3
  $ hg log -r tip
  changeset:   4:67d5c96d65a7
  bookmark:    master_bookmark
  tag:         tip
  parent:      2:1e43292ffbb3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     onemorecommit
  
Make a commit that makes a file executable and a commit that adds a symlink. Make sure they are sync correctly
  $ cd $TESTTMP/client-push
  $ hgmn up -q 2
  $ chmod +x pushcommit
  $ hg ci -m 'exec mode'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 15776eb106e6 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ hgmn up -q 2
  $ ln -s pushcommit symlink_to_pushcommit
  $ hg addremove
  adding symlink_to_pushcommit
  $ hg ci -m 'symlink'
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 6f060fabc8e7 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Continue replay
  $ cd $TESTTMP/repo-hg-3
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python "$TESTTMP/replayverification.py"
  > CONFIG

  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 5
  * using repo "repo" repoid RepositoryId(0) (glob)
  * syncing log entry #6 ... (glob)
  * successful sync (glob)
  * syncing log entry #7 ... (glob)
  * successful sync (glob)
  $ cd repo-hg-3
  $ hg log -r master_bookmark^
  changeset:   5:a7acac33c050
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     exec mode
  
  $ hg log -r master_bookmark
  changeset:   6:6f24f1b38581
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     symlink
  
