#require git no-windows

  $ eagerepo
  $ setconfig diff.git=True
  $ setconfig subtree.cheap-copy=False
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

Prepare a git repo:

  $ . $TESTDIR/git.sh
  $ git init -q gitrepo
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

  $ hg subtree import --url file://$TESTTMP/gitrepo --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path foo -m "import gitrepo to foo"
  abort: cannot import to an existing path: foo
  (use --force to overwrite (recursively remove foo))
  [255]

Test subtree import the root path of the external repo

  $ hg subtree import --url file://$TESTTMP/gitrepo --rev d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 --to-path bar -m "import gitrepo to bar"
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         4487c56011495a40ce2f6c632c24ae57a210747d -> remote/master
   * [new ref]         d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0 -> refs/visibleheads/d2a8b3fa3dfa345ea64e02ea014d21b5cabd03e0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  copying / to bar
  $ hg st --change .
  A bar/alpha
  A bar/dir1/beta
  $ hg log -G -T '{node|short} {desc}\n'
  @  * import gitrepo to bar (glob)
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A

Test subtree import a sub directory of the external repo

  $ hg subtree import --url file://$TESTTMP/gitrepo --rev 4487c56011495a40ce2f6c632c24ae57a210747d --from-path dir2 --to-path mydir2 -m "import gitrepo/dir2 to mydir2"
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         4487c56011495a40ce2f6c632c24ae57a210747d -> remote/master
   * [new ref]         4487c56011495a40ce2f6c632c24ae57a210747d -> refs/visibleheads/4487c56011495a40ce2f6c632c24ae57a210747d
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  copying dir2 to mydir2
  $ hg st --change .
  A mydir2/gamma
  $ hg log -G -T '{node|short} {desc}\n'
  @  ef1d307cba34 import gitrepo/dir2 to mydir2
  │
  o  7410edae120d import gitrepo to bar
  │
  o  9998a5c40732 B
  │
  o  d908813f0f7c A
