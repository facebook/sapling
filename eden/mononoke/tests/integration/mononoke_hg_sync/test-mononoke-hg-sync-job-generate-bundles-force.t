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

  $ hginit_treemanifest repo
  $ cd repo
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
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
Make client repo
  $ hg clone -q mono:repo client-push --noupdate

Push to Mononoke
  $ cd $TESTTMP/client-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF
  $ hg up -q tip

Two pushes, one with --force. Pushes intentionally modify the same file
  $ hg up -q master_bookmark
  $ echo 1 > file_to_conflict
  $ hg addremove -q
  $ hg ci -m 'normal push'
  $ hg push -r . --to master_bookmark -q

  $ hg up -q "master_bookmark^"
  $ echo 11 > file_to_conflict
  $ hg addremove -q
  $ hg ci -m 'force push'
  $ hg push -r . --to master_bookmark -q --force

Move backward
  $ hg push -r .^ --to master_bookmark --force --pushvar NON_FAST_FORWARD=true
  pushing rev add0c792bfce to destination mono:repo bookmark master_bookmark
  searching for changes
  no changes found
  updating bookmark master_bookmark

Sync it to another client
  $ cd $TESTTMP/repo
  $ enable_replay_verification_hook
  $ cd $TESTTMP

Sync a push and a force push
  $ mononoke_hg_sync_loop_regenerate repo 1 --bundle-prefetch 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [2]* (glob)
  * successful sync of entries [3]* (glob)
  * successful sync of entries [4]* (glob)
