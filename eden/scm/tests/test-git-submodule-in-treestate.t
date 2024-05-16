#require git no-windows no-eden

This test forces a submodule path to be tracked in treestate (which does not
usually happen, but has been witnessed), and check related logic can handle
that.

  $ . $TESTDIR/git.sh
  $ export HGIDENTITY=sl  # for drawdag

Prepare smaller submodules

  $ git init -q -b main sub1
  $ drawdag --cwd sub1 << 'EOS'
  > A-B
  > EOS
  $ sl bookmark --cwd sub1 -r $B main 

Prepare git parent repo
(commit hashes are unstable because '.gitmodules' contains TESTTMP paths)

  $ git init -q -b main parent-repo-git
  $ cd parent-repo-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1 mod1
  $ git commit -qm 'add .gitmodules'

Prepare sl parent repo

  $ cd
  $ sl clone -q --git "$TESTTMP/parent-repo-git" parent-repo-sl

Force insert a non-empty 'sub1' state to treestate
(not yet sure how to enter this state without using debugshell...)

  $ cd parent-repo-sl
  $ sl dbsh << 'EOS'
  > with repo.lock(), repo.transaction('test'):
  >     repo.dirstate._addpath('mod1', 'n', 0o40755, 111, 222)
  > EOS

`status` seems okay:

  $ sl status

`commit --addremove` is okay:

  $ touch x
  $ sl commit -m 'add x' --addremove
  adding x

  $ sl status --change .
  A x
  $ sl status
