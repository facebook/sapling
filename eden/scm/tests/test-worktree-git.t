
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

Changes to common and per-worktree Git config refresh the translated config.

  $ git config --local user.name Common
  $ git config --local user.email common@example.com
  $ touch -t 203001010000 ../repo/.git/config
  $ sl config ui.username
  Common <common@example.com>

  $ git config extensions.worktreeConfig true
  $ sl config ui.username
  Common <common@example.com>
  $ git config --worktree user.name Linked
  $ git config --worktree user.email linked@example.com
  $ touch -t 203101010000 ../repo/.git/worktrees/linked/config.worktree
  $ sl config ui.username
  Linked <linked@example.com>

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

Add creates a detached checkout; remove still requires EdenFS.

  $ sl worktree add $TESTTMP/another
  created linked worktree at $TESTTMP/another
  $ sl worktree remove $TESTTMP/another
  abort: worktree commands require an EdenFS-backed repository
  [255]

Git support can be disabled independently from EdenFS worktrees.

  $ sl worktree list --config worktree.git-enabled=false
  abort: worktree commands for git are disabled by worktree.git-enabled
  [255]

Labels work before Sapling has been used in the main checkout.

  $ cd $TESTTMP
  $ git init -qb main labels
  $ cd labels
  $ git commit --allow-empty -qm init
  $ git worktree add -q $TESTTMP/labels-linked --detach
  $ cd $TESTTMP/labels-linked
  $ test ! -e $TESTTMP/labels/.git/sl
  $ sl worktree label linked-label
  label set for $TESTTMP/labels-linked
  $ sl worktree label $TESTTMP/labels main-label
  label set for $TESTTMP/labels
  $ cat $TESTTMP/labels/.git/worktrees/labels-linked/sl/worktreename
  linked-label (no-eol)
  $ cat $TESTTMP/labels/.git/sl/worktreename
  main-label (no-eol)
  $ sl worktree list
    main    $TESTTMP/labels   main-label
  * linked  $TESTTMP/labels-linked   linked-label

Labels follow a checkout that Git moves.

  $ cd $TESTTMP/labels
  $ git worktree move $TESTTMP/labels-linked $TESTTMP/labels-moved
  $ sl worktree list
  * main    $TESTTMP/labels   main-label
    linked  $TESTTMP/labels-moved   linked-label

Labels can be removed from another checkout; an unrelated repository is rejected.

  $ sl worktree label $TESTTMP/labels-moved --remove
  label removed for $TESTTMP/labels-moved
  $ test ! -e $TESTTMP/labels/.git/worktrees/labels-linked/sl/worktreename
  $ sl worktree label $TESTTMP/repo wrong
  abort: '$TESTTMP/repo' is not in this worktree group
  [255]
  $ sl worktree label disabled --config worktree.git-enabled=false
  abort: worktree commands for git are disabled by worktree.git-enabled
  [255]

A label is deleted with its checkout, even if Git later reuses the admin
directory name.

  $ sl worktree label $TESTTMP/labels-moved moved-label
  label set for $TESTTMP/labels-moved
  $ git worktree remove $TESTTMP/labels-moved
  $ sl worktree list
  * main    $TESTTMP/labels   main-label
  $ git worktree add -q $TESTTMP/labels-linked --detach
  $ sl worktree list
  * main    $TESTTMP/labels   main-label
    linked  $TESTTMP/labels-linked
