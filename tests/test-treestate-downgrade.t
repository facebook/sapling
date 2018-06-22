#require fsmonitor

Fsmonitor is required for treestate to track untracked files.

Nonnormalset, otherparentset, copymap might have reference to untracked files.
They should be filtered out when downgrading from treestate to treedirstate.

Create a treestate repo

  $ hg init repo1 --config format.dirstate=2
  $ cd repo1
  $ touch x

Write the untracked file to treestate

  $ hg status
  ? x
  $ hg debugtree
  dirstate v2 (* 1 files tracked) (glob)

Downgrade to treedirstate

  $ hg debugtree v1

Check nonnormalset

  $ hg debugshell --command 'print(repr(sorted(repo.dirstate._map.nonnormalset)))'
  ['x']

BUG: x should not be part of the nonnormalset.

Check downgrade with "hg pull"

  $ hg init $TESTTMP/repo2 --config format.dirstate=2
  $ cd $TESTTMP/repo2
  $ touch x
  $ hg ci -m init -A x -q

  $ hg init $TESTTMP/repo3 --config format.dirstate=2
  $ cd $TESTTMP/repo3
  $ hg pull ../repo2 --config format.dirstate=1 --config treedirstate.migrateonpull=1 --config extensions.rebase= --rebase 2>err
  downgrading dirstate format...
  [1]
  $ tail -1 err
  mercurial.error.ProgrammingError: getclock is only supported by treestate

BUG: crashes with "pull --rebase" downgrade
