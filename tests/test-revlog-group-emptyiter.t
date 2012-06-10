Issue1678: IndexError when pushing

setting up base repo
  $ hg init a
  $ cd a
  $ touch a
  $ hg ci -Am a
  adding a
  $ cd ..

cloning base repo
  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b

setting up cset to push
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch a
different msg so we get a clog new entry
  $ hg ci -Am b
  adding a
  created new head

pushing
  $ hg push -f ../a
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)

  $ cd ..
