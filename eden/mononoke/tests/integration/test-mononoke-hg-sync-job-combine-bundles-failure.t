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
  $ mononoke_hg_sync_loop_regenerate repo-hg 1 --combine-bundles 2 --bundle-prefetch 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [2, 3]* (glob)

  $ cd "$TESTTMP"/repo-hg
  $ hg log -r tip -T '{desc}\n'
  commit_second

One more push, and reset the counter backwards.
This simulates the situation when the previous run of hg sync job failed to
update the "latest replayed" counter. We want to make sure we just skip the first entry in the batch
  $ cd $TESTTMP/client-push
  $ hg up -q master_bookmark
  $ mkcommit commit_third
  $ hgmn push -r . --to master_bookmark -q
  $ mkcommit commit_fourth
  $ hgmn push -r . --to another_book --create -q

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "replace into mutable_counters (repo_id, name, value) values(0, 'latest-replayed-request', 2)"
  $ mononoke_hg_sync_loop_regenerate repo-hg 1 --combine-bundles 2 --bundle-prefetch 2 2>&1 | grep 'adjusting'
  * adjusting first batch - skipping first entries [3]* (glob)

  $ cd "$TESTTMP"/repo-hg

  $ hg log -r master_bookmark -T '{desc}\n'
  commit_third
  $ hg log -r another_book -T '{desc}\n'
  commit_fourth

Now let's simulate the case when repo-hg is a bit behind the source of truth
(e.g. it didn't sync with hgsql yet) and returns outdated version of bookmarks.
  $ cat > $TESTTMP/modifylistkeys.py <<EOF
  > from edenscm.mercurial import (
  >     extensions,
  >     localrepo,
  > )
  > def wraplistkeys(orig, namespace, patterns):
  >     res = orig(namespace, patterns)
  >     if namespace == "bookmarks":
  >         res.remove("another_book")
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, 'listkeys', wraplistkeys)
  > EOF
  $ cd "$TESTTMP"/repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > modifylistkeys = $TESTTMP/modifylistkeys.py
  > EOF

Check that extension was imported fine (i.e. nothing is printed to stderr)
  $ hg log -r tip > /dev/null


  $ cd $TESTTMP/client-push
  $ mkcommit commit_fifth
  $ hgmn push -r . --to another_book --create -q
  $ mkcommit commit_sixth
  $ hgmn push -r . --to another_book --create -q
  $ mononoke_hg_sync_loop_regenerate repo-hg 1 --combine-bundles 2 --bundle-prefetch 2 2>&1 | grep "adjust"
  * trying to adjust first batch for bookmark another_book. First batch starts points to Some(ChangesetId(Blake2(*)))* (glob)
  * could not adjust first batch, * (glob)

  $ cd "$TESTTMP"/repo-hg
  $ hg log -r another_book -T '{desc}\n'
  commit_sixth
