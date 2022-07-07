# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  $ mononoke_hg_sync repo-hg 1 2>&1 | grep 'successful sync'
  * successful sync of entries [2]* (glob)

  $ mononoke_hg_sync repo-hg 2 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)

  $ mononoke_hg_sync repo-hg 3 2>&1 | grep 'successful sync'
  * successful sync of entries [4]* (glob)

  $ mononoke_hg_sync repo-hg 4 2>&1 | grep 'successful sync'
  * successful sync of entries [5]* (glob)

  $ mononoke_hg_sync_loop_regenerate repo-hg 5  2>&1 | grep 'successful sync'
  * successful sync of entries [6]* (glob)
  * successful sync of entries [7]* (glob)

Do a manual move
  $ cd "$TESTTMP/client-push"
  $ NODE="$(hg log -T '{node}' -r master_bookmark~1)"
  $ echo "$NODE"
  f5fb745185a2d197d092e7dfffe147f36de1af76
  $ mononoke_admin bookmarks set master_bookmark "$NODE" &> /dev/null
  $ cd "$TESTTMP"
  $ mononoke_hg_sync_loop_regenerate repo-hg 6 2>&1 | grep 'successful sync'
  * successful sync of entries [8]* (glob)
  $ cd "$TESTTMP/repo-hg"
  $ hg log -r master_bookmark -T '{node}\n'
  f5fb745185a2d197d092e7dfffe147f36de1af76
