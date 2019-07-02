  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob:files"
  $ export CENSORING_DISABLED=1
  $ setup_common_config $REPOTYPE

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > remotenames =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hgmn push -q -r .  --to master_bookmark

  $ hg log -T '{node}\n'
  14961831bd3af3a6331fef7e63367d61cb6c9f6b
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90

  $ cd "$TESTTMP/repo-pull"

  $ hgmn pull -q
  $ tglogpnr
  o  14961831bd3a public 'add b' master_bookmark
  |
  o  ac82d8b1f7c4 public 'add a'
  


Censor the blacklisted blob (file 'b' in commit '14961831bd3af3a6331fef7e63367d61cb6c9f6b')
  $ mononoke_admin blacklist --hash 14961831bd3af3a6331fef7e63367d61cb6c9f6b --task "my_task" b
  * INFO using repo "repo" repoid RepositoryId(0) (glob)

Restart mononoke
  $ kill $MONONOKE_PID
  $ rm -rf $TESTTMP/mononoke-config
  $ setup_common_config blob:files
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  14961831bd3a public 'add b' master_bookmark
  |
  o  ac82d8b1f7c4 public 'add a'
  


  $ hgmn up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
