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
  $ sqlite3 "$TESTTMP/repo/bookmarks" "select count(*) from bookmarks_update_log";
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
  * preparing log entry #1 ... (glob)
  * queue size after processing: 3 (glob)
  * running a failure handler: "$TESTTMP/onfailure.sh" (glob)
  Failure handling.
  * finished running a failure handler (glob)
  * sync failed for ids [1] (glob)
  * caused by: unexpected bookmark move: blobimport (glob)

Sync a pushrebase bookmark move
  $ mononoke_hg_sync repo-hg 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 2 (glob)
  * successful sync of entries [2] (glob)
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
  * preparing log entry #3 ... (glob)
  * successful prepare of entry #3 (glob)
  * syncing log entries [3] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     6cc06ef82eeb  anothercommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 1 (glob)
  * successful sync of entries [3] (glob)
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
  * preparing log entry #4 ... (glob)
  * successful prepare of entry #4 (glob)
  * syncing log entries [4] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [4] (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  changeset:   2:1e43292ffbb3
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
Sync with incorrect timestamps, make sure replay fails
  $ cd $TESTTMP

Use the same code here as in the actual opsfiles hook
  $ cat >>$TESTTMP/replayverification.py <<EOF
  > def verify_replay(ui, repo, *args, **kwargs):
  >     EXP_ONTO = "EXPECTED_ONTOBOOK"
  >     EXP_HEAD = "EXPECTED_REBASEDHEAD"
  > 
  >     expected_book = kwargs.get(EXP_ONTO)
  >     expected_head = kwargs.get(EXP_HEAD)
  >     actual_book = kwargs.get("key")
  >     actual_head = kwargs.get("new")
  >     allowed_replay_books = ui.configlist("facebook", "hooks.unbundlereplaybooks", [])
  >     # If there is a problem with the mononoke -> hg sync job we need a way to
  >     # quickly disable the replay verification to let unsynced bundles
  >     # through.
  >     # Disable this hook by placing a file in the .hg directory.
  >     if repo.localvfs.exists('REPLAY_BYPASS'):
  >         ui.note("[ReplayVerification] Bypassing check as override file is present\n")
  >         return 0
  >     if expected_book is None and expected_head is None:
  >         # We are allowing non-unbundle-replay pushes to go through
  >         return 0
  > 
  >     if allowed_replay_books and actual_book not in allowed_replay_books:
  >         ui.warn("[ReplayVerification] only allowed to unbundlereplay on %r\n" % (allowed_replay_books, ))
  >         return 1
  >     expected_head = expected_head or None
  >     actual_head = actual_head or None
  >     if expected_book == actual_book and expected_head == actual_head:
  >        ui.note("[ReplayVerification] Everything seems in order\n")
  >        return 0
  > 
  >     ui.warn("[ReplayVerification] Expected: (%s, %s). Actual: (%s, %s)\n" % (expected_book, expected_head, actual_book, actual_head))
  >     return 1
  > EOF

  $ cat >> $TESTTMP/repo_lock.py << EOF
  > def run(*args, **kwargs):
  >     """Repo is locked for everything except replays
  >     In-process style hook."""
  >     if kwargs.get("EXPECTED_ONTOBOOK"):
  >         return 0
  >     print "[RepoLock] Repo locked for non-unbundlereplay pushes"
  >     return 1
  > EOF

  $ cd repo-hg-2
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python:$TESTTMP/replayverification.py:verify_replay
  > prepushkey.lock = python:$TESTTMP/repo_lock.py:run
  > [facebook]
  > hooks.unbundlereplaybooks=other_bookmark
  > CONFIG
  $ hg log -r master_bookmark
  changeset:   1:add0c792bfce
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
  $ cd $TESTTMP
  $ sqlite3 "$TESTTMP/repo/bookmarks" "update bundle_replay_data set commit_hashes_json = '{\"1e43292ffbb38fa183e7f21fb8e8a8450e61c890\":10000000000}' where bookmark_update_log_id = 2"
  $ mononoke_hg_sync_with_retry repo-hg-2 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] only allowed to unbundlereplay on ['other_bookmark']
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  * sync failed, let's check if the bookmark is where we want it to be anyway (glob)
  connected to * (glob)
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: master_bookmark, server: add0c792bfce89610d277fd5b1e32f5287994d1d; expected 1e43292ffbb38fa183e7f21fb8e8a8450e61c890
  * retrying attempt 2 of 3... (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] only allowed to unbundlereplay on ['other_bookmark']
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  * sync failed, let's check if the bookmark is where we want it to be anyway (glob)
  connected to * (glob)
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: master_bookmark, server: add0c792bfce89610d277fd5b1e32f5287994d1d; expected 1e43292ffbb38fa183e7f21fb8e8a8450e61c890
  * retrying attempt 3 of 3... (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] only allowed to unbundlereplay on ['other_bookmark']
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  * sync failed, let's check if the bookmark is where we want it to be anyway (glob)
  connected to * (glob)
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: master_bookmark, server: add0c792bfce89610d277fd5b1e32f5287994d1d; expected 1e43292ffbb38fa183e7f21fb8e8a8450e61c890
  * queue size after processing: 2 (glob)
  * sync failed for ids [2] (glob)
  * caused by: sync failed: hg logs follow: (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] only allowed to unbundlereplay on ['other_bookmark']
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  
Oops, we allowed a wrong bookmark to be unbundlereplayed onto
  $ cat >> $TESTTMP/repo-hg-2/.hg/hgrc << CONFIG
  > [facebook]
  > hooks.unbundlereplaybooks=master_bookmark,blabla
  > CONFIG

Now bookmark is not blocked
  $ mononoke_hg_sync repo-hg-2 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] Expected: (master_bookmark, 1e43292ffbb38fa183e7f21fb8e8a8450e61c890). Actual: (master_bookmark, acc06228d802cbe9e2a6740c0abacf017f3be65c)
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  * sync failed, let's check if the bookmark is where we want it to be anyway (glob)
  connected to * (glob)
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server does not have an expected bookmark location. book: master_bookmark, server: add0c792bfce89610d277fd5b1e32f5287994d1d; expected 1e43292ffbb38fa183e7f21fb8e8a8450e61c890
  * queue size after processing: 2 (glob)
  * sync failed for ids [2] (glob)
  * caused by: sync failed: hg logs follow: (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  remote: [ReplayVerification] Expected: (master_bookmark, 1e43292ffbb38fa183e7f21fb8e8a8450e61c890). Actual: (master_bookmark, acc06228d802cbe9e2a6740c0abacf017f3be65c)
  remote: pushkey-abort: prepushkey hook failed
  remote: transaction abort!
  remote: rollback completed
  replay failed: error:pushkey
  

Set the correct timestamp back
  $ sqlite3 "$TESTTMP/repo/bookmarks" "update bundle_replay_data set commit_hashes_json = '{\"1e43292ffbb38fa183e7f21fb8e8a8450e61c890\":0}' where bookmark_update_log_id = 2"

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
  * preparing log entry #1 ... (glob)
  * queue size after processing: 3 (glob)
  * sync failed for ids [1] (glob)
  * unexpected bookmark move: blobimport (glob)
  $ mononoke_hg_sync_loop repo-hg-3 1 --bundle-prefetch 0
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-3 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 2 (glob)
  * successful sync of entries [2] (glob)
  * preparing log entry #3 ... (glob)
  * successful prepare of entry #3 (glob)
  * syncing log entries [3] ... (glob)
  single wireproto command took: * (glob)
  sending unbundlereplay command
  remote: * (glob)
  remote: * (glob)
  unbundle replay batch item #1 successfully sent
  * queue size after processing: 1 (glob)
  * successful sync of entries [3] (glob)
  * preparing log entry #4 ... (glob)
  * successful prepare of entry #4 (glob)
  * syncing log entries [4] ... (glob)
  single wireproto command took: * (glob)
  sending unbundlereplay command
  unbundle replay batch item #2 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [4] (glob)
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
  * preparing log entry #5 ... (glob)
  * successful prepare of entry #5 (glob)
  * syncing log entries [5] ... (glob)
  running * 'hg -R repo-hg-3 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     67d5c96d65a7  onemorecommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [5] (glob)
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
  * preparing log entry #6 ... (glob)
  * successful prepare of entry #6 (glob)
  * syncing log entries [6] ... (glob)
  running * 'hg -R repo-hg-3 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     15776eb106e6  exec mode
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 1 (glob)
  * successful sync of entries [6] (glob)
  * preparing log entry #7 ... (glob)
  * successful prepare of entry #7 (glob)
  * syncing log entries [7] ... (glob)
  single wireproto command took: * (glob)
  sending unbundlereplay command
  remote: * (glob)
  remote:     6f060fabc8e7  symlink
  unbundle replay batch item #1 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [7] (glob)
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
  
Verify that repo-hg-2 is locked for normal pushes
  $ cd $TESTTMP/client-push
  $ hg up 0 -q
  $ echo >> ababagalamaga && hg ci -qAm ababagalamaga
  $ hg push -r . --to master_bookmark ssh://user@dummy/repo-hg-2
  pushing rev 24e27c11427d to destination ssh://user@dummy/repo-hg-2 bookmark master_bookmark
  searching for changes
  remote: pushing 1 changeset:
  remote:     24e27c11427d  ababagalamaga
  remote: 1 new changeset from the server will be downloaded
  remote: [RepoLock] Repo locked for non-unbundlereplay pushes
  remote: pushkey-abort: prepushkey.lock hook failed
  remote: transaction abort!
  remote: rollback completed
  abort: updating bookmark master_bookmark failed!
  [255]

Test hook bypass using REPLAY_BYPASS file
  $ cd $TESTTMP/repo-hg-2
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python:$TESTTMP/replayverification.py:verify_replay
  > [facebook]
  > hooks.unbundlereplaybooks=other_bookmark
  > CONFIG
  $ hg log -r master_bookmark
  changeset:   1:add0c792bfce
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
  $ cd $TESTTMP
  $ touch repo-hg-2/.hg/REPLAY_BYPASS

Test failing to sync, but already having the correct bookmark location
  $ sqlite3 "$TESTTMP/repo/bookmarks" "update bundle_replay_data set commit_hashes_json = '{\"add0c792bfce89610d277fd5b1e32f5287994d1d\":10000000000}' where bookmark_update_log_id = 2"
  $ mononoke_hg_sync_with_retry repo-hg-2 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 5 (glob)
  * successful sync of entries [2] (glob)

Test further sync
  $ sqlite3 "$TESTTMP/repo/bookmarks" "update bundle_replay_data set commit_hashes_json = '{\"1e43292ffbb38fa183e7f21fb8e8a8450e61c890\":10000000000}' where bookmark_update_log_id = 2"
  $ mononoke_hg_sync_with_retry repo-hg-2 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg-2 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  replay failed: error:abort
  part message: conflicting changes in:
      pushcommit
  unbundle replay batch item #0 failed
  * sync failed, let's check if the bookmark is where we want it to be anyway (glob)
  connected to * (glob)
  creating a peer took: * (glob)
  running lookup took: * (glob)
  hg server has expected bookmark location. book: master_bookmark, hash: 1e43292ffbb38fa183e7f21fb8e8a8450e61c890
  * queue size after processing: 5 (glob)
  * successful sync of entries [2] (glob)

Test bookmark deletion sync
  $ cat >>$TESTTMP/repo-hg-3/.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python:$TESTTMP/replayverification.py:verify_replay
  > CONFIG
  $ cd $TESTTMP/client-push
  $ hgmn -q up master_bookmark
  $ hgmn -q push --rev . --to book_to_delete --create
  [1]
  $ hg log -r master_bookmark
  changeset:   8:6f24f1b38581
  bookmark:    default/book_to_delete
  bookmark:    default/master_bookmark
  hoistedname: book_to_delete
  hoistedname: master_bookmark
  parent:      6:a7acac33c050
  user:        test
  date:        * (glob)
  summary:     symlink
  
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 7
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #8 ... (glob)
  * successful prepare of entry #8 (glob)
  * syncing log entries [8] ... (glob)
  running * serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [8] (glob)
  $ cd $TESTTMP/client-push
  $ hgmn push --delete book_to_delete
  pushing to * (glob)
  searching for changes
  no changes found
  deleting remote bookmark book_to_delete
  [1]
  $ hg log -r master_bookmark
  changeset:   8:6f24f1b38581
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  parent:      6:a7acac33c050
  user:        test
  date:        * (glob)
  summary:     symlink
  
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 8
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #9 ... (glob)
  * successful prepare of entry #9 (glob)
  * syncing log entries [9] ... (glob)
  running * 'hg -R repo-hg-3 serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [9] (glob)
  $ cd $TESTTMP/repo-hg-3
  $ hg log -r master_bookmark
  changeset:   6:6f24f1b38581
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        * (glob)
  summary:     symlink
  
Test the job exits when the exit file is set
  $ cd $TESTTMP/client-push
  $ hg up -q master_bookmark
  $ mkcommit exitcommit
  $ hgmn push -r . --to master_bookmark -q
  $ touch $TESTTMP/exit-file
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 8 --exit-file $TESTTMP/exit-file
  * using repo "repo" repoid RepositoryId(0) (glob)
  * path "$TESTTMP/exit-file" exists: exiting ... (glob)
