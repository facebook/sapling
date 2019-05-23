  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ echo 1 > 1 && hg addremove && hg ci -m 1
  adding 1
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 2 > 2 && hg addremove && hg ci -m 2
  adding 2

Clone the repo
  $ cd ..
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cd ../repo-hg

Create merge commit with rename
  $ hg up -q 0
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg mv 1 2 --force
  $ hg ci -m merge
  $ hg st --change . -C
  A 2
    1
  R 1

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

  $ cd repo2
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (-1 heads)
  adding remote bookmark master_bookmark
  new changesets 38674c683e74
  $ hgmn up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st --change . -C
  A 2
    1
  R 1
