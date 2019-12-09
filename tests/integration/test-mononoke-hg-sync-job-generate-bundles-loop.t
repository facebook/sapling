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
  $ mononoke_hg_sync_loop_regenerate repo-hg 1 --bundle-prefetch 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [2] (glob)
  * successful sync of entries [3] (glob)

New bookmark is created
  $ cd $TESTTMP/client-push
  $ hg up -q 0
  $ mkcommit newbook_commit_first
  $ hgmn push -r . --to newbook -q --create

  $ hg up -q newbook
  $ mkcommit newbook_commit_second
  $ hgmn push -r . --to newbook -q

Sync a pushrebase bookmark move. Note that we are using 0 start-id intentionally - sync job
should ignore it and fetch the latest id from db
  $ cd $TESTTMP
  $ mononoke_hg_sync_loop_regenerate repo-hg 0 --bundle-prefetch 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [4] (glob)
  * successful sync of entries [5] (glob)
