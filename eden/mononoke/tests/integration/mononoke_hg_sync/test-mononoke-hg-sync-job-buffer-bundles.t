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

Two pushes synced one after another
  $ hg up -q master_bookmark
  $ mkcommit commit_1
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_2
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_3
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_4
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_5
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_6
  $ hg push -r . --to master_bookmark -q

  $ hg up -q master_bookmark
  $ mkcommit commit_7
  $ hg push -r . --to master_bookmark -q
Sync it to another client
  $ cd $TESTTMP/repo
  $ enable_replay_verification_hook
  $ cd $TESTTMP

Sync a pushrebase bookmark move
  $ mononoke_hg_sync_loop_regenerate repo 1 --combine-bundles 2 --bundle-buffer-size 2 2>&1 | grep 'ful sync\|prepare' | cut -d " "  -f 6- > out

(the actual syncs need to happen in-order)
  $ cat out | grep sync
  successful sync of entries [2, 3], repo: repo
  successful sync of entries [4, 5], repo: repo
  successful sync of entries [6, 7], repo: repo
  successful sync of entries [8], repo: repo

(but the preparation of log entries doesn't have to be in order so we sort it)
  $ cat out | grep prepare | sort
  successful prepare of entries #[2, 3], repo: repo
  successful prepare of entries #[4, 5], repo: repo
  successful prepare of entries #[6, 7], repo: repo
  successful prepare of entries #[8], repo: repo

  $ cd "$TESTTMP"/repo
  $ hg log -r tip -T '{desc}\n'
  commit_7
