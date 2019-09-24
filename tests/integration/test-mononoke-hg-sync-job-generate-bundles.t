  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ cp "${TEST_FIXTURES}/pushrebase_replay.bundle" "$TESTTMP/handle"
  $ create_pushrebaserecording_sqlite3_db
  $ init_pushrebaserecording_sqlite3_db
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

Sync it to another client
  $ cd $TESTTMP
  $ cat >> repo-hg/.hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF

Sync a pushrebase bookmark move
  $ mononoke_hg_sync repo-hg 1 --generate-bundles
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #2 ... (glob)
  * successful prepare of entry #2 (glob)
  * syncing log entries [2] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     1e43292ffbb3  pushcommit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: * (glob)
  * successful sync of entries [2] (glob)

  $ mononoke_hg_sync repo-hg 2 --generate-bundles
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #3 ... (glob)
  * successful prepare of entry #3 (glob)
  * syncing log entries [3] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 2 changesets:
  remote:     7468ab807774  pushcommit2
  remote:     8c820fb6ee4a  pushcommit3
  unbundle replay batch item #0 successfully sent
  * queue size after processing: * (glob)
  * successful sync of entries [3] (glob)

  $ mononoke_hg_sync repo-hg 3 --generate-bundles
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #4 ... (glob)
  * successful prepare of entry #4 (glob)
  * syncing log entries [4] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 2 changesets:
  remote:     f99e423f3833  modify 1
  remote:     b4ac84388288  modify 1
  unbundle replay batch item #0 successfully sent
  * queue size after processing: * (glob)
  * successful sync of entries [4] (glob)

  $ mononoke_hg_sync repo-hg 4 --generate-bundles
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #5 ... (glob)
  * successful prepare of entry #5 (glob)
  * syncing log entries [5] ... (glob)
  running * 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities* (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: * (glob)
  single wireproto command took: * (glob)
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 2 changesets:
  remote:     d428c6e5b373  empty
  remote:     fa88034ff7f8  empty
  unbundle replay batch item #0 successfully sent
  * queue size after processing: * (glob)
  * successful sync of entries [5] (glob)
