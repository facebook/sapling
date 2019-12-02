  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a" > a
  $ echo "b" > b
  $ hg addremove && hg ci -q -ma
  adding a
  adding b
  $ hg log -T '{node}\n'
  0cd96de13884b090099512d4794ae87ad067ea8e

create master bookmark
  $ hg bookmark master_bookmark -r tip

setup repo-push and repo-pull
  $ cd $TESTTMP
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke

push some files with copy/move files

  $ cd $TESTTMP/repo-push
  $ hg up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ hg cp a a_copy
  $ hg mv b b_move
  $ hg addremove && hg ci -q -mb
  recording removal of b as rename to b_move (100% similar)
  $ hgmn push ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  searching for changes
  updating bookmark master_bookmark

pull them

  $ cd $TESTTMP/repo-pull
  $ hg up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ hg log -T '{node}\n'
  0cd96de13884b090099512d4794ae87ad067ea8e
  $ hgmn pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets 4b747ca852a4
  $ hg log -T '{node}\n'
  4b747ca852a40a105b9bb71cd4d07248ea80f704
  0cd96de13884b090099512d4794ae87ad067ea8e

push files that modify copied and moved files

  $ cd $TESTTMP/repo-push
  $ echo "aa" >> a_copy
  $ echo "bb" >> b_move
  $ hg addremove && hg ci -q -mc
  $ hgmn push ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  searching for changes
  updating bookmark master_bookmark

pull them

  $ cd $TESTTMP/repo-pull
  $ hg log -T '{node}\n'
  4b747ca852a40a105b9bb71cd4d07248ea80f704
  0cd96de13884b090099512d4794ae87ad067ea8e
  $ hgmn pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets 8b374fd7e2ef
  $ hg log -T '{node}\n'
  8b374fd7e2ef1cc418b9c68f484ebd2cb6c6c6a1
  4b747ca852a40a105b9bb71cd4d07248ea80f704
  0cd96de13884b090099512d4794ae87ad067ea8e
  $ hgmn up master_bookmark
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat a_copy
  a
  aa
  $ cat b_move
  b
  bb
