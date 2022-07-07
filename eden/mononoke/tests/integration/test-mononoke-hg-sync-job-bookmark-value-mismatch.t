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

Move bookmark to another position, and make sure hg sync job fails because of
pushkey erro
  $ cd $TESTTMP/repo-hg
  $ hg book master_bookmark -r "min(all())" -f

  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 2 2>&1 | grep 'error:pushkey'
  replay failed: error:pushkey
  * replay failed: error:pushkey (glob)

Now make sure it replays correctly
  $ mononoke_hg_sync repo-hg 2 --use-hg-server-bookmark-value-if-mismatch 2>&1 | grep 'master_bookmark'
  * master_bookmark is expected to point to *, but it actually points to * on hg server. Forcing master_bookmark to point to * (glob)
  $ cd $TESTTMP/repo-hg
  $ hg log -r master_bookmark
  commit:      * (glob)
  bookmark:    master_bookmark
  user:        test
  date:        * (glob)
  summary:     pushcommit3
  
