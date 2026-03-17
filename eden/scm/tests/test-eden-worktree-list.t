
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

test worktree list - plain output from main (shows * on main)

  $ hg worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
    linked  $TESTTMP/linked_from_subdir
  * main    $TESTTMP/myrepo

test worktree list - json output

  $ hg worktree list -Tjson | hg debugpython -- -c "
  > import json, sys
  > data = json.load(sys.stdin)
  > print(len(data))
  > roles = [e['role'] for e in data]
  > print('main' in roles)
  > print(roles.count('linked'))
  > currents = [e['current'] for e in data]
  > print(sum(currents))
  > "
  4
  True
  3
  1

test worktree list - from linked worktree (* on linked)

  $ cd $TESTTMP/linked1
  $ hg worktree list
  * linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
    linked  $TESTTMP/linked_from_subdir
    main    $TESTTMP/myrepo
  $ cd $TESTTMP/myrepo

test worktree list - not in a group

  $ newclientrepo isolated_repo
  $ touch f.txt && hg add f.txt && hg commit -m "init"
  $ hg worktree list
  this worktree is not part of a group
  $ cd $TESTTMP/myrepo

test worktree list - group isolation

  $ newclientrepo myrepo2
  $ touch file2.txt && hg add file2.txt && hg commit -m "init2"
  $ hg worktree add $TESTTMP/linked_repo2
  created linked worktree at $TESTTMP/linked_repo2
  $ hg worktree list
    linked  $TESTTMP/linked_repo2
  * main    $TESTTMP/myrepo2
  $ cd $TESTTMP/myrepo
  $ hg worktree list
    linked  $TESTTMP/linked1
    linked  $TESTTMP/linked2   feature-x
    linked  $TESTTMP/linked_from_subdir
  * main    $TESTTMP/myrepo
