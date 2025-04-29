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
  $ echo "1\n2\n3\n4\n5\n" > a.txt
  $ git add a.txt
  $ git commit -q -m G1

  $ echo "1a\n2\n3\n4\n5\n" > a.txt
  $ git add .
  $ git commit -q -m G2

  $ echo "1a\n2\n3a\n4\n5\n" > a.txt
  $ git add .
  $ git commit -q -m G3

  $ git log --graph
  * commit 1ac30162f86b42e7c4e4effdf4d6dab2032483a2
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     G3
  | 
  * commit 01a40e59f19ee93c5782a9cf8ed780e981adc634
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     G2
  | 
  * commit 9aadc4795874831ab6dc2f77d11ca6f69c3f6fab
    Author: test <test@example.org>
    Date:   Mon Jan 1 00:00:10 2007 +0000
    
        G1

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

Test subtre prefetch
  $ hg subtree prefetch --url $GIT_URL --rev 9aadc4795874831ab6dc2f77d11ca6f69c3f6fab
  creating git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         1ac30162f86b42e7c4e4effdf4d6dab2032483a2 -> remote/master
   * [new ref]         9aadc4795874831ab6dc2f77d11ca6f69c3f6fab -> refs/visibleheads/9aadc4795874831ab6dc2f77d11ca6f69c3f6fab
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
