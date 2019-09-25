  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
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
  $ wait_for_mononoke $TESTTMP/repo

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
  $ hgmn push -r . --to newbook --create -q

Sync it to another client
  $ cd $TESTTMP
  $ cat >> repo-hg/.hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF

Sync a creation of a bookmark
  $ mononoke_hg_sync repo-hg 1 --generate-bundles
  * using repo "repo" repoid RepositoryId(0) (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: * (glob)
  * successful sync of entries [2] (glob)
