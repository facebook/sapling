# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

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
  $ mononoke
  $ wait_for_mononoke

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
  $ hg up -q 0
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
  $ mononoke_hg_sync repo-hg 1 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [2] (glob)

  $ mononoke_hg_sync repo-hg 2 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [3] (glob)

  $ mononoke_hg_sync repo-hg 3 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [4] (glob)

  $ mononoke_hg_sync repo-hg 4 --generate-bundles 2>&1 | grep 'successful sync'
  * successful sync of entries [5] (glob)

  $ mononoke_hg_sync_loop_regenerate repo-hg 5  2>&1 | grep 'successful sync'
  * successful sync of entries [6] (glob)
  * successful sync of entries [7] (glob)
