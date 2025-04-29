#require git no-windows

  $ eagerepo
  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

Prepare a git repo:

  $ . $TESTDIR/git.sh
  $ git -c init.defaultBranch=main init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ mkdir dir1
  $ echo 2 > dir1/beta
  $ git add dir1/beta
  $ git commit -q -mbeta

  $ mkdir dir2
  $ echo 3 > dir2/gamma
  $ git add dir2/gamma
  $ git commit -q -mgamma

  $ git log --graph
  * commit 4487c56011495a40ce2f6c632c24ae57a210747d
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     gamma
  | 
  * commit d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     beta
  | 
  * commit b6c31add3e60ded7a9c9c803641edffb1dccd251
    Author: test <test@example.org>
    Date:   Mon Jan 1 00:00:10 2007 +0000
    
        alpha

  $ export GIT_URL=git+file://$TESTTMP/gitrepo
  
Prepare a Sapling repo:

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q

Test subtree import failure cases

  $ hg subtree import --url $GIT_URL --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path foo -m "import gitrepo to foo"
  abort: cannot import to an existing path: foo
  (use --force to overwrite)
  [255]
  $ hg subtree import --url $GIT_URL --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0
  abort: must specify the to-path
  [255]
  $ hg subtree import --url $GIT_URL --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --from-path dir1 --to-path bar --from-path dir2 --to-path bar/dir2
  abort: overlapping --to-path entries
  [255]
  $ hg subtree import --url $GIT_URL --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path bar -m "import gitrepo to bar" --config subtree.min-path-depth=2
  abort: path should be at least 2 levels deep: 'bar'
  [255]
  $ hg subtree import --url file://$TESTTMP/gitrepo --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path bar -m "import gitrepo to bar" --config subtree.min-path-depth=2
  abort: unable to determine git url from 'file:/*/$TESTTMP/gitrepo' (glob)
  [255]
  $ hg subtree import --url $GIT_URL --rev b6c31add3e60ded7a9c9c803641edffb1dccd251 --from-path nonexistent_dir --to-path bar
  creating git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         4487c56011495a40ce2f6c632c24ae57a210747d -> remote/main
   * [new ref]         b6c31add3e60ded7a9c9c803641edffb1dccd251 -> refs/visibleheads/b6c31add3e60ded7a9c9c803641edffb1dccd251
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  abort: path 'nonexistent_dir' does not exist in commit b6c31add3e60
  [255]

Test subtree import the root path of the external repo

  $ hg subtree import --url $GIT_URL --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path bar -m "import gitrepo to bar"
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 -> refs/visibleheads/d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0
  copying / to bar
  $ hg st --change .
  A bar/alpha
  A bar/dir1/beta
  $ hg log -G -T '{node|short} {desc}\n'
  @  * import gitrepo to bar (glob)
  │
  │  Subtree import from git+file:/*/$TESTTMP/gitrepo at d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 (glob)
  │  - Imported path / to bar
  o  9998a5c40732 B
  │
  o  d908813f0f7c A

Test subtree import a sub directory of the external repo

  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"imports":[{"from_commit":"d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0","from_path":"","to_path":"bar","url":"git+file:/*/$TESTTMP/gitrepo"}],"v":1}] (glob)

  $ hg subtree import --url $GIT_URL --rev 4487c56011495a40ce2f6c632c24ae57a210747d --from-path dir2 --to-path mydir2 -m "import gitrepo/dir2 to mydir2"
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         4487c56011495a40ce2f6c632c24ae57a210747d -> refs/visibleheads/4487c56011495a40ce2f6c632c24ae57a210747d
  copying dir2 to mydir2
  $ hg st --change .
  A mydir2/gamma
  $ hg log -G -T '{node|short} {desc}\n'
  @  * import gitrepo/dir2 to mydir2 (glob)
  │
  │  Subtree import from git+file:/*/$TESTTMP/gitrepo at 4487c56011495a40ce2f6c632c24ae57a210747d (glob)
  │  - Imported path /dir2 to mydir2
  o  * import gitrepo to bar (glob)
  │
  │  Subtree import from git+file:/*/$TESTTMP/gitrepo at d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 (glob)
  │  - Imported path / to bar
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"imports":[{"from_commit":"4487c56011495a40ce2f6c632c24ae57a210747d","from_path":"dir2","to_path":"mydir2","url":"git+file:/*/$TESTTMP/gitrepo"}],"v":1}] (glob)

  $ hg fold --from .^
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st --change .
  A bar/alpha
  A bar/dir1/beta
  A mydir2/gamma
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"imports":[{"from_commit":"d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0","from_path":"","to_path":"bar","url":"git+file://$TESTTMP/gitrepo"},{"from_commit":"4487c56011495a40ce2f6c632c24ae57a210747d","from_path":"dir2","to_path":"mydir2","url":"git+file:/*/$TESTTMP/gitrepo"}],"v":1}] (glob)
  $ hg subtree inspect
  {
    "imports": [
      {
        "version": 1,
        "url": "git+file:/*/$TESTTMP/gitrepo", (glob)
        "from_commit": "d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0",
        "from_path": "",
        "to_path": "bar"
      },
      {
        "version": 1,
        "url": "git+file:/*/$TESTTMP/gitrepo", (glob)
        "from_commit": "4487c56011495a40ce2f6c632c24ae57a210747d",
        "from_path": "dir2",
        "to_path": "mydir2"
      }
    ]
  }

Test subtree import with branch name

  $ newclientrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg go $A -q
  $ hg subtree import --url $GIT_URL --rev main --to-path bar -m "import gitrepo to bar"
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  copying / to bar
  $ hg subtree inspect
  {
    "imports": [
      {
        "version": 1,
        "url": "git+file:/*/$TESTTMP/gitrepo", (glob)
        "from_commit": "4487c56011495a40ce2f6c632c24ae57a210747d",
        "from_path": "",
        "to_path": "bar"
      }
    ]
  }
