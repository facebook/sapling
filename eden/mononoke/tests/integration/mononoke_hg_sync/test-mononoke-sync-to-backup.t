# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOID=0 REPONAME=orig setup_common_config blob_files
  $ REPOID=1 REPONAME=backup setup_common_config blob_files
  $ export BACKUP_REPO_ID=1
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
  $ REPOID=0 blobimport repo-hg/.hg orig
  $ REPONAME=orig
  $ REPOID=1 blobimport repo-hg/.hg backup

start mononoke
  $ start_and_wait_for_mononoke_server
Make client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames= -q
  $ hgclone_treemanifest mononoke://$(mononoke_address)/backup backup --noupdate --config extensions.remotenames=

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
  $ hg up -q master_bookmark
  $ mkcommit pushcommit2
  $ mkcommit pushcommit3
  $ hgmn push -r . --to master_bookmark -q

Modify same file
  $ hg up -q master_bookmark
  $ echo 1 >> 1 && hg addremove && hg ci -m 'modify 1'
  adding 1
  $ echo 1 >> 1 && hg addremove && hg ci -m 'modify 1'
  $ hgmn push -r . --to master_bookmark -q

Empty commits
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg -q addremove && hg ci -m empty
  $ hg revert -r ".^" 1 && hg commit --amend

  $ echo 1 > 1 && hg -q addremove && hg ci -m empty
  $ hg revert -r ".^" 1 && hg commit --amend

  $ hgmn push -r . --to master_bookmark -q

Two pushes synced one after another
  $ hg up -q master_bookmark
  $ mkcommit commit_first
  $ hgmn push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_second
  $ hgmn push -r . --to master_bookmark -q

Sync it to another client
  $ cd $TESTTMP/repo-hg
  $ enable_replay_verification_hook
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ cd $TESTTMP

Sync a pushrebase bookmark move
  $ mononoke_backup_sync backup sync-once 1 2>&1 | grep 'successful sync'
  * successful sync of entries [2]* (glob)

  $ mononoke_backup_sync backup sync-once 2 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)

  $ mononoke_backup_sync backup sync-once 3 2>&1 | grep 'successful sync'
  * successful sync of entries [4]* (glob)

  $ mononoke_backup_sync backup sync-loop 4 2>&1 | grep 'successful sync'
  * successful sync of entries [5]* (glob)
  * successful sync of entries [6]* (glob)
  * successful sync of entries [7]* (glob)


Do a manual move
  $ cd "$TESTTMP/client-push"
  $ TIP="$(hg log -T '{node}' -r master_bookmark)"
  $ TIP_PARENT="$(hg log -T '{node}' -r master_bookmark~1)"

  $ mononoke_newadmin bookmarks --repo-id=0 set master_bookmark "$TIP_PARENT"
  Updating publishing bookmark master_bookmark from aa56217d7c6265a0624bfdc78047bd26d6189e9f2667f9a41e6a51ca80c30a3c to 8e7d2998fd68c6efaae7db7e93fdbaa73d34123ceedebc082acdb5ab955c5f2a

  $ cd "$TESTTMP"
  $ mononoke_backup_sync backup sync-loop 2 --bookmark-move-any-direction 2>&1 | grep 'successful sync'
  * successful sync of entries [8]* (glob)

  $ cd "$TESTTMP/backup"
  $ REPONAME=backup
  $ hgmn pull
  pulling from mononoke://$LOCALIP:*/backup (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding remote bookmark master_bookmark
  $ hgmn log -r master_bookmark -T '{node}\n'
  f5fb745185a2d197d092e7dfffe147f36de1af76
  $ echo "$TIP_PARENT"
  f5fb745185a2d197d092e7dfffe147f36de1af76

Move forward to a commit that's already present in the destination
  $ mononoke_newadmin bookmarks --repo-id=0 set master_bookmark "$TIP"
  Updating publishing bookmark master_bookmark from 8e7d2998fd68c6efaae7db7e93fdbaa73d34123ceedebc082acdb5ab955c5f2a to aa56217d7c6265a0624bfdc78047bd26d6189e9f2667f9a41e6a51ca80c30a3c

  $ cd "$TESTTMP"
  $ mononoke_backup_sync backup sync-loop 2 --bookmark-move-any-direction 2>&1 | grep -e 'successful sync' -e 'already in the darkstorm backup repo'
  * 1 of 1 commits already in the darkstorm backup repo, not including them in the bundle, repo: orig (glob)
  * successful sync of entries [9], repo: orig (glob)

  $ cd "$TESTTMP/backup"
  $ REPONAME=backup
  $ hgmn pull
  pulling from mononoke://$LOCALIP:*/backup (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hgmn log -r master_bookmark -T '{node}\n'
  bcf523b814e2cbae2d4d2d5b1cbbe3e391f4b4d8
  $ echo "$TIP"
  bcf523b814e2cbae2d4d2d5b1cbbe3e391f4b4d8

Make sure correct mutable counter is used (it should be repoid = 1)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters" | grep latest
  1|latest-replayed-request|9
