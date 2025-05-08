#require git no-windows

  $ enable morestatus
  $ setconfig morestatus.show=True
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

  $ git mv a.txt b.txt
  $ git add .
  $ git commit -q -m G4

  $ git log --graph
  * commit e815a5c1f80404f40f8fe492f461e91b4cc0e976
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     G4
  | 
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
  $ hg subtree prefetch --url $GIT_URL --rev aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  creating git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  fatal: git upload-pack: not our ref aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  fatal: remote error: upload-pack: not our ref aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  abort: unknown revision 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'!
  [255]

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
   * [new ref]         e815a5c1f80404f40f8fe492f461e91b4cc0e976 -> remote/main
subtree graft a range of commits should work
  $ hg subtree graft --url $GIT_URL --rev 0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185::2d03d263ac7869815998b556ccec69eb36edebda --from-path "" --to-path mygitrepo
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  grafting 0e0bbd7f53d7 "G2"
  grafting 2d03d263ac78 "G3"
  $ hg log -G -T '{node|short} {desc}\n' -p -r .^::
  @  516708be4743 Graft "G3"
  │
  │  Grafted from 2d03d263ac7869815998b556ccec69eb36edebda
  │  - Grafted path  to mygitrepo
  │  diff --git a/mygitrepo/a.txt b/mygitrepo/a.txt
  │  --- a/mygitrepo/a.txt
  │  +++ b/mygitrepo/a.txt
  │  @@ -1,5 +1,5 @@
  │   1a
  │   2
  │  -3
  │  +3a
  │   4
  │   5
  │
  o  ccd4e9b5eba6 Graft "G2"
  │
  ~  Grafted from 0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185
     - Grafted path  to mygitrepo
     diff --git a/mygitrepo/a.txt b/mygitrepo/a.txt
     --- a/mygitrepo/a.txt
     +++ b/mygitrepo/a.txt
     @@ -1,4 +1,4 @@
     -1
     +1a
      2
      3
      4
  $ hg subtree graft --url $GIT_URL --rev e815a5c1f80404f40f8fe492f461e91b4cc0e976 --from-path "" --to-path mygitrepo
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  grafting e815a5c1f804 "G4"
XXX: handle cross-repo copy tracing
  $ hg log -G -T '{node|short} {desc}\n' -p -r .
  @  dc73c8c4b2ea Graft "G4"
  │
  ~  Grafted from e815a5c1f80404f40f8fe492f461e91b4cc0e976
     - Grafted path  to mygitrepo
     diff --git a/mygitrepo/a.txt b/mygitrepo/a.txt
     deleted file mode 100644
     --- a/mygitrepo/a.txt
     +++ /dev/null
     @@ -1,5 +0,0 @@
     -1a
     -2
     -3a
     -4
     -5
     diff --git a/mygitrepo/b.txt b/mygitrepo/b.txt
     new file mode 100644
     --- /dev/null
     +++ b/mygitrepo/b.txt
     @@ -0,0 +1,5 @@
     +1a
     +2
     +3a
     +4
     +5

Test subtree graft with merge conflicts
  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/a.txt = 1b\n2\n3\n4\n5\n
  > |
  > A   # A/foo/a.txt = 1\n2\n3\n4\n5\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q
  $ hg subtree graft --url $GIT_URL --rev 0e0bbd7f53d7f8dfa9ef6283f68e2aa5d274a185 --from-path "" --to-path foo
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  grafting 0e0bbd7f53d7 "G2"
  merging foo/a.txt and a.txt to foo/a.txt
  warning: 1 conflicts while merging foo/a.txt! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg st
  M foo/a.txt
  ? foo/a.txt.orig
  
  # The repository is in an unfinished *graft* state.
  # Unresolved merge conflicts (1):
  # 
  #     foo/a.txt
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  # To continue:                hg graft --continue
  # To abort:                   hg graft --abort
  $ hg diff
  diff --git a/foo/a.txt b/foo/a.txt
  --- a/foo/a.txt
  +++ b/foo/a.txt
  @@ -1,4 +1,8 @@
  +<<<<<<< local: 3fccbb413558 - test: B
   1b
  +=======
  +1a
  +>>>>>>> graft: 0e0bbd7f53d7 - test: G2
   2
   3
   4
  $ echo "1a1b\n2\n3a\n4\n5" > foo/a.txt
  $ hg resolve --all --mark
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft --continue
  grafting 0e0bbd7f53d7 "G2"
  $ hg log -r . -p
  commit:      c914ffc85eb4
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     G2
  
  diff --git a/foo/a.txt b/foo/a.txt
  --- a/foo/a.txt
  +++ b/foo/a.txt
  @@ -1,5 +1,5 @@
  -1b
  +1a1b
   2
  -3
  +3a
   4
   5
