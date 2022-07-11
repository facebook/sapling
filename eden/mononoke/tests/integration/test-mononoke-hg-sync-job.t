# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration

  $ setup_common_config blob_files
  $ cd $TESTTMP

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
  $ start_and_wait_for_mononoke_server
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

Check that new entry was added to the sync database. 3 pushes and 1 blobimport
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from bookmarks_update_log";
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
  $ cd "$TESTTMP"

State 0 means Mononoke is unlocked
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "insert into repo_lock (repo_id, state, reason) values(0, 0, null)";
  $ mononoke_hg_sync_with_failure_handler repo-hg 0 2>&1 | grep 'unexpected bookmark move'
  * unexpected bookmark move: blobimport* (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) from repo_lock"
  1

Sync a pushrebase bookmark move
  $ mononoke_hg_sync repo-hg 1 2>&1 | grep 'successful sync'
  * successful sync of entries [2]* (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  commit:      1e43292ffbb3
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 2 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  commit:      6cc06ef82eeb
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     anothercommit
  
  $ hg log -r master_bookmark^
  commit:      1e43292ffbb3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 3 2>&1 | grep 'successful sync'
  * successful sync of entries [4]* (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark
  commit:      1e43292ffbb3
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushcommit
  
Enable replay verification hooks
  $ cd $TESTTMP/repo-hg-2
  $ enable_replay_verification_hook
  $ hg log -r master_bookmark
  commit:      add0c792bfce
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
  $ cd $TESTTMP
  $ cd repo-hg-2
  $ hg log -r master_bookmark
  commit:      add0c792bfce
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
Replay in a loop
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 0 2>&1 | grep 'unexpected bookmark'
  * unexpected bookmark move: blobimport* (glob)
  *] Execution error: unexpected bookmark move: blobimport (glob)
  $ mononoke_hg_sync_loop repo-hg-3 1 --bundle-prefetch 0 2>&1 | grep 'successful sync'
  * successful sync of entries [2]* (glob)
  * successful sync of entries [3]* (glob)
  * successful sync of entries [4]* (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'latest-replayed-request'";
  0|latest-replayed-request|4

Unlock the repo so that we can push again
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE repo_lock SET state = 0 WHERE repo_id = 0";

Make one more push from the client
  $ cd $TESTTMP/client-push
  $ hg up -q master_bookmark
  $ mkcommit onemorecommit
  $ hgmn push -r . --to master_bookmark -q

Continue replay
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 1 2>&1 | grep 'successful sync'
  * successful sync of entries [5]* (glob)
  $ cd $TESTTMP/repo-hg-3
  $ hg log -r tip
  commit:      67d5c96d65a7
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     onemorecommit
  
Make a commit that makes a file executable and a commit that adds a symlink. Make sure they are sync correctly
  $ cd $TESTTMP/client-push
  $ hgmn up -q 2
  $ chmod +x pushcommit
  $ hg ci -m 'exec mode'
  $ hgmn push -r . --to master_bookmark
  pushing rev 15776eb106e6 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hgmn up -q 2
  $ ln -s pushcommit symlink_to_pushcommit
  $ hg addremove
  adding symlink_to_pushcommit
  $ hg ci -m 'symlink'
  $ hgmn push -r . --to master_bookmark
  pushing rev 6f060fabc8e7 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Continue replay
  $ cd $TESTTMP/repo-hg-3
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python "$TESTTMP/replayverification.py"
  > CONFIG

  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 5 2>&1 | grep 'successful sync'
  * successful sync of entries [6]* (glob)
  * successful sync of entries [7]* (glob)
  $ cd repo-hg-3
  $ hg log -r master_bookmark^
  commit:      a7acac33c050
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     exec mode
  
  $ hg log -r master_bookmark
  commit:      6f24f1b38581
  bookmark:    master_bookmark
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
  commit:      add0c792bfce
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a => bar
  
  $ cd $TESTTMP
  $ touch repo-hg-2/.hg/REPLAY_BYPASS

Test failing to sync, but already having the correct bookmark location
  $ mononoke_hg_sync_with_retry repo-hg-2 1 2>&1 | grep 'successful sync'
  * successful sync of entries [2]* (glob)

Test further sync
  $ mononoke_hg_sync_with_retry repo-hg-2 1 2>&1 | grep -E '(sync failed|successful sync)'
  * successful sync of entries [2]* (glob)

Test bookmark deletion sync
  $ cat >>$TESTTMP/repo-hg-3/.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python:$TESTTMP/replayverification.py:verify_replay
  > CONFIG
  $ cd $TESTTMP/client-push
  $ hgmn -q up master_bookmark
  $ hgmn -q push --rev . --to book_to_delete --create
  $ hg log -r master_bookmark
  commit:      6f24f1b38581
  bookmark:    default/book_to_delete
  bookmark:    default/master_bookmark
  hoistedname: book_to_delete
  hoistedname: master_bookmark
  user:        test
  date:        * (glob)
  summary:     symlink
  
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 7 2>&1 | grep 'successful sync'
  * successful sync of entries [8]* (glob)
  $ cd $TESTTMP/client-push
  $ hgmn push --delete book_to_delete
  pushing to * (glob)
  searching for changes
  no changes found
  deleting remote bookmark book_to_delete
  [1]
  $ hg log -r master_bookmark
  commit:      6f24f1b38581
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        * (glob)
  summary:     symlink
  
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 8 2>&1 | grep 'successful sync'
  * successful sync of entries [9]* (glob)
  $ cd $TESTTMP/repo-hg-3
  $ hg log -r master_bookmark
  commit:      6f24f1b38581
  bookmark:    master_bookmark
  user:        test
  date:        * (glob)
  summary:     symlink
  

Test force pushrebase sync
  $ cd $TESTTMP/client-push
  $ hgmn -q up master_bookmark^
-- create a commit, which is not an ancestor of master
  $ mkcommit commit_to_force_pushmaster
  $ hg log -r .
  commit:      cc83c88b72d3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit_to_force_pushmaster
  
-- force-pushrebase this commit
  $ hgmn push -q -f --to master_bookmark
-- master should now point to it
  $ hg log -r .
  commit:      cc83c88b72d3
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit_to_force_pushmaster
  
-- let us now see if we can replay it
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 8 2>&1 | grep 'successful sync'
  * successful sync of entries [10]* (glob)
-- and if the replay result is good (e.g. master_bookmark points to the same commit as in client-push)
  $ cd $TESTTMP/repo-hg-3
  $ hg log -r master_bookmark
  commit:      cc83c88b72d3
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit_to_force_pushmaster
  

Test the job exits when the exit file is set
  $ cd $TESTTMP/client-push
  $ hg up -q master_bookmark
  $ mkcommit exitcommit
  $ hgmn push -r . --to master_bookmark -q
  $ touch $TESTTMP/exit-file
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop repo-hg-3 8 --exit-file $TESTTMP/exit-file 2>&1 | grep 'exists'
  * path "$TESTTMP/exit-file" exists: exiting ...* (glob)
