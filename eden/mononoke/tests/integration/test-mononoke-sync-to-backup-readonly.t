# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOID=0 REPONAME=orig setup_common_config blob_files
  $ export READ_ONLY_REPO=1
  $ REPOID=1 REPONAME=backup setup_common_config blob_files
  $ export BACKUP_REPO_ID=1
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ cd ..

Blobimport the hg repo to Mononoke
  $ REPOID=0 blobimport repo-hg/.hg orig
  $ REPONAME=orig
  $ REPOID=1 blobimport repo-hg/.hg backup

start mononoke
  $ start_and_wait_for_mononoke_server
Push to Mononoke
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames=
  $ cd $TESTTMP/client-push
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > [treemanifest]
  > treeonly=True
  > EOF
  $ hg up -q tip

  $ mkcommit pushcommit
  $ hgmn push -r . --to master_bookmark -q

Sync it to another client should fail, because of readonly repo
  $ cd $TESTTMP
  $ mononoke_backup_sync backup sync-once 2 2>&1 | grep 'Repo is locked' | sed -e 's/^[ ]*//' | sort --unique
  * Repo is locked: Set by config option (glob)


Sync it to another client with bypass-readonly should success
  $ cd $TESTTMP
  $ mononoke_backup_sync backup sync-once 2 --bypass-readonly 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)

Check synced commit in backup repo
  $ hgclone_treemanifest mononoke://$(mononoke_address)/backup backup --noupdate --config extensions.remotenames=
  $ cd "$TESTTMP/backup"
  $ REPONAME=backup
  $ hgmn pull -q
  $ hgmn log -r master_bookmark -T '{node}\n'
  9fdce596be1b7052b777aa0bf7c5e87b00397a6f
