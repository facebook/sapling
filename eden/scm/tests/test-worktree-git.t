
#require git no-windows no-eden

  $ . $TESTDIR/git.sh

  $ git init -qb main repo
  $ cd repo
  $ echo a > a
  $ git add a
  $ git commit -qm init
  $ git worktree add -q ../linked -b feature

  $ sl worktree list
  * main    $TESTTMP/repo
    linked  $TESTTMP/linked

  $ cd ../linked
  $ sl worktree list
    main    $TESTTMP/repo
  * linked  $TESTTMP/linked

  $ sl worktree list -Tjson | sl debugpython -- -c "
  > import json, sys
  > data = json.load(sys.stdin)
  > print([entry['role'] for entry in data])
  > print([entry['current'] for entry in data])
  > "
  ['main', 'linked']
  [False, True]

Other operations remain limited to EdenFS-backed repositories.

  $ sl worktree add ../another
  abort: worktree commands require an EdenFS-backed repository
  [255]

Git support can be disabled independently from EdenFS worktrees.

  $ sl worktree list --config worktree.git-enabled=false
  abort: worktree commands for git are disabled by worktree.git-enabled
  [255]
