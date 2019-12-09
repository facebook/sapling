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

Two pushes, one with --force. Pushes intentionally modify the same file
  $ hg up -q master_bookmark
  $ echo 1 > file_to_conflict
  $ hg addremove -q
  $ hg ci -m 'normal push'
  $ hgmn push -r . --to master_bookmark -q

  $ hg up -q "master_bookmark^"
  $ echo 11 > file_to_conflict
  $ hg addremove -q
  $ hg ci -m 'force push'
  $ hgmn push -r . --to master_bookmark -q --force

Move backward
  $ hgmn push -r .^ --to master_bookmark --force --pushvar NON_FAST_FORWARD=true
  pushing rev add0c792bfce to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  no changes found
  updating bookmark master_bookmark
  [1]

Sync it to another client
  $ cd $TESTTMP/repo-hg
  $ enable_replay_verification_hook
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ cd $TESTTMP

Sync a push and a force push
  $ mononoke_hg_sync_loop_regenerate repo-hg 1 --bundle-prefetch 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [2] (glob)
  * successful sync of entries [3] (glob)
  * successful sync of entries [4] (glob)
