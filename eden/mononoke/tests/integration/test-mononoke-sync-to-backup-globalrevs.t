# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark REPOID=0 REPONAME=orig setup_common_config blob_files
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
  $ hg up -q master_bookmark

  $ mkcommit pushcommit
  $ hgmn push -r . --to master_bookmark -q
  $ hg up -q master_bookmark
  $ mkcommit pushcommit2
  $ mkcommit pushcommit3
  $ hgmn push -r . --to master_bookmark -q

Sync to backup repos
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select repo_id, globalrev from bonsai_globalrev_mapping"
  0|1000147970
  0|1000147971
  0|1000147972

  $ mononoke_backup_sync backup sync-loop 2 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)
  * successful sync of entries [4]* (glob)


Make sure correct mutable counter is used (it should be repoid = 1)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters" | grep latest
  1|latest-replayed-request|4
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select repo_id, globalrev from bonsai_globalrev_mapping"
  0|1000147970
  0|1000147971
  0|1000147972
  1|1000147970
  1|1000147971
  1|1000147972
