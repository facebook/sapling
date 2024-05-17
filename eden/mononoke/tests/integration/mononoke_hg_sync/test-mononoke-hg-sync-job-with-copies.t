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
  $ start_and_wait_for_mononoke_server
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
  $ hg up -q "min(all())"
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
  $ mononoke_hg_sync repo-hg 2 2>&1 | grep 'successful sync'
  * successful sync of entries [3]* (glob)
  $ cd repo-hg
  $ hg log -r master_bookmark -T '{node}\n'
  a1e678b3ed9a3df8ef590d407b97d88891a66778

Push of a merge with a copy
  $ cd $TESTTMP/client-push

  $ hg up -q "min(all())"
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
  commit:      bc6bfc6ac632
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
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
  commit:      bc6bfc6ac632
  bookmark:    master_bookmark
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
  e09d568b9a5530903dcc9e4a2a60b1912141379c
  $ hgmn st --change tip -C
  M ancestorscase
  A somefile


Second diamond push, this time "ancestorscase2" is modified in the second
parent 

  $ hgmn up -q master_bookmark
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
  c019126b122e679401c27e13131609aa50d3e806

  $ cd $TESTTMP
  $ mononoke_hg_sync repo-hg 4 &> /dev/null
Sync merges
  $ mononoke_hg_sync repo-hg 5 &>/dev/null
  $ mononoke_hg_sync repo-hg 6 &>/dev/null
  $ mononoke_hg_sync repo-hg 7 &>/dev/null
  $ cd $TESTTMP/repo-hg
  $ hg log -r tip -T '{node}\n'
  c019126b122e679401c27e13131609aa50d3e806
