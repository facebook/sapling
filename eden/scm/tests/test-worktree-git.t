
#require git no-windows no-eden

  $ . $TESTDIR/git.sh

  $ git init -qb main repo
  $ cd repo
  $ echo a > a
  $ git add a
  $ git commit -qm init
  $ git worktree add -q ../linked -b feature

The first Sapling command can run from a linked worktree. It creates local
working-copy state under GIT_DIR and shared store state under GIT_COMMON_DIR.

  $ test ! -e .git/sl
  $ cd ../linked
  $ sl root --dotdir
  $TESTTMP/repo/.git/worktrees/linked/sl
  $ sl root --shared --dotdir
  $TESTTMP/repo/.git/sl
  $ test -f ../repo/.git/worktrees/linked/sl/dirstate
  $ test -f ../repo/.git/sl/store/requires
  $ cat ../repo/.git/sl/store/gitdir
  ../.. (no-eol)
  $ cat ../repo/.git/worktrees/linked/sl/bookmarks.current
  feature (no-eol)

Python uses the common Sapling dir for shared state and the store, while the
shared working-copy root remains the linked worktree.

  $ sl debugpython -- -c "
  > from sapling import hg, ui as uimod
  > repo = hg.repository(uimod.ui.load(), '.')
  > print(repo.path)
  > print(repo.sharedpath)
  > print(repo.store_path)
  > print(repo.sharedroot)
  > "
  $TESTTMP/repo/.git/worktrees/linked/sl
  $TESTTMP/repo/.git/sl
  $TESTTMP/repo/.git/sl/store
  $TESTTMP/linked

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

The main worktree gets separate local state but reuses the store initialized by
the linked worktree.

  $ cd ../repo
  $ sl root --dotdir
  $TESTTMP/repo/.git/sl
  $ cat .git/sl/bookmarks.current
  main (no-eol)
  $ sl worktree list
  * main    $TESTTMP/repo
    linked  $TESTTMP/linked

Repo config remains local to a worktree. Store metadata is shared.

  $ cat >> .git/sl/config << 'EOF'
  > [test]
  > shared = main
  > EOF
  $ echo b > b
  $ sl commit -qAm b
  $ sl amend -qm b2
  $ sl config test.shared
  main
  $ sl log -r 'obsolete()' --hidden -T '{desc}\n'
  b
  $ sl debugmetalogroots -v | grep -q amend
  $ sl debugsegmentgraph > $TESTTMP/main-segments
  $ test -d .git/sl/store/segments/v1
  $ test ! -e .git/worktrees/linked/sl/store/segments

  $ cd ../linked
  $ sl config test.shared
  [1]
  $ sl log -r 'obsolete()' --hidden -T '{desc}\n'
  b
  $ sl debugmetalogroots -v | grep -q amend
  $ sl debugsegmentgraph > $TESTTMP/linked-segments
  $ cmp $TESTTMP/main-segments $TESTTMP/linked-segments
  $ test ! -e ../repo/.git/worktrees/linked/sl/store/segments

The working copy parent still comes from the worktree's own HEAD.

  $ sl log -r . -T '{desc}\n'
  init

The common dir can be a bare repo without a working copy.

  $ git clone -q --bare ../repo ../bare.git
  $ git -C ../bare.git worktree add -q ../from-bare main
  $ cd ../from-bare
  $ sl log -r . -T '{desc}\n'
  b2
  $ sl root --shared --dotdir
  $TESTTMP/bare.git/sl
  $ test -f ../bare.git/sl/store/requires
  $ test ! -e ../bare.git/sl/dirstate
  $ cd ../linked

Other operations remain limited to EdenFS-backed repositories.

  $ sl worktree add ../another
  abort: worktree commands require an EdenFS-backed repository
  [255]

Git support can be disabled independently from EdenFS worktrees.

  $ sl worktree list --config worktree.git-enabled=false
  abort: worktree commands for git are disabled by worktree.git-enabled
  [255]
