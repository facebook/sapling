  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ ENABLE_PRESERVE_BUNDLE2=1 setup_common_config blob:files
  $ cp "$TEST_FIXTURES/pushrebase_replay.bundle" "$TESTTMP/handle"
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
  running * 'user@dummy' 'hg -R repo-hg serve --stdio' (glob)
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

Push of a merge with a copy
  $ cd $TESTTMP/client-push

  $ hg up -q 0
  $ echo 1 > fromcopyremote
  $ echo 1 > notinfirstparent
  $ hg addremove -q && hg ci -m tomerge
  $ COMMIT=$(hg log -r tip -T '{node}')

  $ hg up -q master_bookmark
  $ echo 1 > fromcopylocal
  $ hg addremove -q && hg ci -m mergeinto
  $ hg merge -q $COMMIT
  $ hg cp fromcopyremote remotecopied
  $ hg cp fromcopylocal localcopied
  $ echo 2 > notinfirstparent
  $ hg ci -m 'copied'
  $ hgmn push -r . --to master_bookmark -q
  $ hg log -r tip
  changeset:   9:bc6bfc6ac632
  tag:         tip
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  parent:      8:af1639811192
  parent:      7:21ecc753c272
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     copied
  

  $ hgmn st --change tip -C
  A fromcopyremote
  A localcopied
    fromcopylocal
  A notinfirstparent
  A remotecopied

  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 3 &> /dev/null
  $ cd $TESTTMP/repo-hg
  $ hg log -r tip
  changeset:   7:bc6bfc6ac632
  bookmark:    master_bookmark
  tag:         tip
  parent:      6:af1639811192
  parent:      5:21ecc753c272
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     copied
  

  $ hg st --change tip -C
  A fromcopyremote
  A localcopied
    fromcopylocal
  A notinfirstparent
  A remotecopied

Merge when one filenode is ancestor of another
  $ cd $TESTTMP/client-push

  $ hg up -q master_bookmark
  $ INITIALCOMMIT=$(hg log -r tip -T '{node}')
  $ echo 1 >> 1
  $ hg ci -m 'some commit'
  $ hgmn push -r . --to master_bookmark -q

Make 4 commits arranged in a diamond shape
"ancestorscase" file is created in the start commit,
modified in one of the merged parents and in the merge commit itself
  $ hg up -q $INITIALCOMMIT
  $ echo 1 > ancestorscase
  $ hg addremove -q && hg ci -m initial
  $ STARTCOMMIT=$(hg log -r tip -T '{node}')

  $ echo 2 > ancestorscase
  $ hg addremove -q && hg ci -m firstparent
  $ FIRSTPARENT=$(hg log -r tip -T '{node}')

  $ hg up -q $STARTCOMMIT
  $ echo 1 > somefile
  $ hg addremove -q && hg ci -m secondparent
  $ SECONDPARENT=$(hg log -r tip -T '{node}')
  $ hg up -q $FIRSTPARENT
  $ hg merge -q $SECONDPARENT
  $ echo 3 > ancestorscase
  $ hg ci -m 'ancestors'
  $ hgmn push -r . --to master_bookmark -q
  $ hg log -r tip -T '{node}\n'
  aca179fa10740cb530e81a2d0ada525c2026ca2c
  $ hgmn st --change tip -C
  M ancestorscase
  A somefile


Second diamond push, this time "ancestorscase2" is modified in the second
parent 

  $ hg up -q master_bookmark
  $ INITIALCOMMIT=$(hg log -r tip -T '{node}')
  $ echo 1 >> 1
  $ hg ci -m 'some commit'
  $ hgmn push -r . --to master_bookmark -q

  $ hg up -q $INITIALCOMMIT
  $ echo 1 > ancestorscase2
  $ hg addremove -q && hg ci -m initial
  $ STARTCOMMIT=$(hg log -r tip -T '{node}')

  $ echo 1 > somefile2
  $ hg addremove -q && hg ci -m firstparent
  $ FIRSTPARENT=$(hg log -r tip -T '{node}')

  $ hg up -q $STARTCOMMIT
  $ echo 2 > ancestorscase2
  $ hg addremove -q && hg ci -m secondparent
  $ SECONDPARENT=$(hg log -r tip -T '{node}')
  $ hg up -q $FIRSTPARENT
  $ hg merge -q $SECONDPARENT
  $ echo 3 > ancestorscase2
  $ hg ci -m 'ancestors'
  $ hgmn push -r . --to master_bookmark -q
  $ hg log -r tip -T '{node}\n'
  b4e3aff8dd1842190c0d0cfbf180ea5190d1211e

  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 4 &> /dev/null
Sync first "diamond" push
  $ mononoke_hg_sync repo-hg 5 2>&1 | grep ReplayVerification
  [1]

  $ mononoke_hg_sync repo-hg 6 &> /dev/null
Sync second "diamond" push
  $ mononoke_hg_sync repo-hg 7 2>&1 | grep ReplayVerification
  [1]
  $ cd $TESTTMP/repo-hg
  $ hg log -r tip -T '{node}\n'
  b4e3aff8dd1842190c0d0cfbf180ea5190d1211e
