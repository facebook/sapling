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
  $ echo "1\n2\n3\n4\n5" > a.txt
  $ git add a.txt
  $ git commit -q -m G1

  $ echo "1a\n2\n3\n4\n5" > a.txt
  $ git add .
  $ git commit -q -m G2

  $ echo "1a\n2\n3a\n4\n5" > a.txt
  $ git add .
  $ git commit -q -m G3

  $ git log --graph
  * commit 2d03d263ac7869815998b556ccec69eb36edebda
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     G3
  | 
  * commit 0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     G2
  | 
  * commit 22cc654c7242ce76728ac8baaab057e3cdf7e024
    Author: test <test@example.org>
    Date:   Mon Jan 1 00:00:10 2007 +0000
    
        G1

  $ export GIT_URL=git+file://$TESTTMP/gitrepo


Test subtree prefetch with an invalid commit hash
  $ newclientrepo
  $ hg subtree prefetch --url $GIT_URL --rev aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2>&1 | head -n 10
  creating git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  fatal: git upload-pack: not our ref aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  fatal: remote error: upload-pack: not our ref aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  abort: unknown revision 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'!

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
  $ hg subtree prefetch --url $GIT_URL --rev 22cc654c7242ce76728ac8baaab057e3cdf7e024
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         22cc654c7242ce76728ac8baaab057e3cdf7e024 -> refs/visibleheads/22cc654c7242ce76728ac8baaab057e3cdf7e024

Test subtree graft
  $ hg subtree graft --url $GIT_URL --rev 22cc654c7242ce76728ac8baaab057e3cdf7e024 --from-path "" --to-path mygitrepo
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  grafting 22cc654c7242 "G1"
  $ hg show
  commit:      d4b49c908230
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  files:       mygitrepo/a.txt
  description:
  Graft "G1"
  
  Grafted from 22cc654c7242ce76728ac8baaab057e3cdf7e024
  - Grafted path  to mygitrepo
  
  
  diff --git a/mygitrepo/a.txt b/mygitrepo/a.txt
  new file mode 100644
  --- /dev/null
  +++ b/mygitrepo/a.txt
  @@ -0,0 +1,5 @@
  +1
  +2
  +3
  +4
  +5

  $ hg subtree prefetch --url $GIT_URL --rev main
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         2d03d263ac7869815998b556ccec69eb36edebda -> remote/main
tofix: subtree graft a range of commits should work
  $ hg subtree graft --url $GIT_URL --rev 0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185:: --from-path "" --to-path mygitrepo
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  grafting 0e0bbd7f53d7 "G2"
  abort: unknown revision '0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185'!
  [255]
