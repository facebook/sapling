  $ . $TESTDIR/library.sh

setup configuration

  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ cp "$TESTDIR/pushrebase_replay.bundle" "$TESTTMP/handle"
  $ create_pushrebaserecording_sqlite3_db
  $ init_pushrebaserecording_sqlite3_db
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo foo > a
  $ echo foo > b
  $ hg ci -Aqm 'initial'
  $ echo 'bar' > a
  $ hg ci -m 'a => bar'
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

Push a simple commit to Mononoke
  $ cd $TESTTMP/client-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hg up -q tip
  $ echo 1 > 1
  $ hg ci -Aqm 'test commit'
  $ hg log -r tip -T '{node}\n'
  f1c370cc51a0684dcc579385cc255882bcdc8bcb
  $ hgmn push -r . --to master_bookmark -q

Push two commits to Mononoke, one of them has a force copy
  $ hg up -q 0
  $ mkdir dir
  $ cd dir
  $ echo 1 > 1 && echo 2 > 2
  $ hg ci -Aqm 'commit'

  $ hg cp 2 1 --force
  $ hg ci -m 'bad commit'
  $ hgmn push -r . --to master_bookmark -q
  $ hg log -r tip -T '{node}\n'
  a1e678b3ed9a3df8ef590d407b97d88891a66778

Sync it to another client
  $ cd $TESTTMP/repo-hg
  $ enable_replay_verification_hook

Sync first simple push
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 1 &> /dev/null
  $ cd repo-hg
  $ hg log -r master_bookmark -T '{node}\n'
  f1c370cc51a0684dcc579385cc255882bcdc8bcb

Sync second tricky push
  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 2
  * using repo "repo" repoid RepositoryId(0) (glob)
  * preparing log entry #3 ... (glob)
  * successful prepare of entry #3 (glob)
  * syncing log entries [3] ... (glob)
  running "*dummyssh.par" 'user@dummy' 'hg -R repo-hg serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 570
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  connected to * (glob)
  creating a peer took: 0.000 ns
  single wireproto command took: 0.000 ns
  using * as a reports file (glob)
  sending unbundlereplay command
  remote: pushing 2 changesets:
  remote:     4e05343c7747  commit
  remote:     0d5aeb697ee7  bad commit
  unbundle replay batch item #0 successfully sent
  * queue size after processing: 0 (glob)
  * successful sync of entries [3] (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark -T '{node}\n'
  a1e678b3ed9a3df8ef590d407b97d88891a66778
