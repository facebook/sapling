
#require eden

  $ setconfig worktree.enabled=true

setup backing repo with linked worktrees

  $ newclientrepo myrepo
  $ touch file.txt
  $ hg add file.txt
  $ hg commit -m "init"
  $ hg worktree add $TESTTMP/linked1
  created linked worktree at $TESTTMP/linked1
  $ hg worktree add $TESTTMP/linked2 --label "feature-x"
  created linked worktree at $TESTTMP/linked2
  $ hg worktree add $TESTTMP/linked_from_subdir
  created linked worktree at $TESTTMP/linked_from_subdir

test worktree remove - missing PATH argument

  $ hg worktree remove
  abort: usage: sl worktree remove PATH
  [255]

test worktree remove - cannot remove main

  $ hg worktree remove $TESTTMP/myrepo -y
  abort: cannot remove '$TESTTMP/myrepo': your current working directory is inside it
  [255]

test worktree remove - basic remove

  $ hg worktree remove $TESTTMP/linked_from_subdir -y
  removed $TESTTMP/linked_from_subdir
  $ test -d $TESTTMP/linked_from_subdir
  [1]

test worktree remove - list after remove shows fewer entries

  $ hg worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
  * main    $TESTTMP/myrepo

test worktree remove - with --keep

  $ hg worktree add $TESTTMP/keep_me
  created linked worktree at $TESTTMP/keep_me
  $ hg worktree remove $TESTTMP/keep_me --keep -y
  unlinked $TESTTMP/keep_me
  $ test -d $TESTTMP/keep_me
  $ hg worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
  * main    $TESTTMP/myrepo

clean up kept checkout
  $ EDENFSCTL_ONLY_RUST=true eden rm -y $TESTTMP/keep_me > /dev/null 2>&1

test worktree remove --all

  $ hg worktree remove --all -y
  removed $TESTTMP/linked1
  removed $TESTTMP/linked2

test worktree remove - group dissolved after all linked removed

  $ hg worktree list
  this worktree is not part of a group
