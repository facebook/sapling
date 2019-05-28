  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
#require symlink execbit

  $ enable amend morestatus purge rebase
  $ setconfig morestatus.show=True
  $ setconfig diff.git=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True

Rebase a simple DAG:
  $ hg init repo1
  $ cd repo1
  $ hg debugdrawdag <<'EOS'
  > c b
  > |/
  > d
  > |
  > a
  > EOS
  $ hg up -C a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglog
  o  3: 814f6bd05178 'c'
  |
  | o  2: db0e82a16a62 'b'
  |/
  o  1: 02952614a83d 'd'
  |
  @  0: b173517d0057 'a'
  
  $ hg cat -r 3 c
  c (no-eol)
  $ hg cat -r 2 b
  b (no-eol)
  $ hg rebase --debug -r b -d c | grep rebasing
  rebasing in-memory
  rebasing 2:db0e82a16a62 "b" (b)
  $ tglog
  o  3: ca58782ad1e4 'b'
  |
  o  2: 814f6bd05178 'c'
  |
  o  1: 02952614a83d 'd'
  |
  @  0: b173517d0057 'a'
  
  $ hg cat -r 3 b
  b (no-eol)
  $ hg cat -r 2 c
  c (no-eol)

Case 2:
  $ hg init repo2
  $ cd repo2
  $ hg debugdrawdag <<'EOS'
  > c b
  > |/
  > d
  > |
  > a
  > EOS

Add a symlink and executable file:
  $ hg up -C c
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ln -s somefile e
  $ echo f > f
  $ chmod +x f
  $ hg add e f
  $ hg amend -q
  $ hg up -Cq a

Write files to the working copy, and ensure they're still there after the rebase
  $ echo "abc" > a
  $ ln -s def b
  $ echo "ghi" > c
  $ echo "jkl" > d
  $ echo "mno" > e
  $ tglog
  o  3: f56b71190a8f 'c'
  |
  | o  2: db0e82a16a62 'b'
  |/
  o  1: 02952614a83d 'd'
  |
  @  0: b173517d0057 'a'
  
  $ hg cat -r 3 c
  c (no-eol)
  $ hg cat -r 2 b
  b (no-eol)
  $ hg cat -r 3 e
  somefile (no-eol)
  $ hg rebase --debug -s b -d a | grep rebasing
  rebasing in-memory
  rebasing 2:db0e82a16a62 "b" (b)
  $ tglog
  o  3: fc055c3b4d33 'b'
  |
  | o  2: f56b71190a8f 'c'
  | |
  | o  1: 02952614a83d 'd'
  |/
  @  0: b173517d0057 'a'
  
  $ hg cat -r 2 c
  c (no-eol)
  $ hg cat -r 3 b
  b (no-eol)
  $ hg rebase --debug -s 1 -d 3 | grep rebasing
  rebasing in-memory
  rebasing 1:02952614a83d "d" (d)
  rebasing 2:f56b71190a8f "c"
  $ tglog
  o  3: 753feb6fd12a 'c'
  |
  o  2: 09c044d2cb43 'd'
  |
  o  1: fc055c3b4d33 'b'
  |
  @  0: b173517d0057 'a'
  
Ensure working copy files are still there:
  $ cat a
  abc
  $ readlink.py b
  b -> def
  $ cat e
  mno

Ensure symlink and executable files were rebased properly:
  $ hg up -Cq 3
  $ readlink.py e
  e -> somefile
  $ ls -l f | cut -c -10
  -rwxr-xr-x
  $ cd ..

Make a change that only changes the flags of a file and ensure it rebases
cleanly.
  $ hg clone repo2 repo3
  updating to branch default
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo3
  $ tglog
  @  3: 753feb6fd12a 'c'
  |
  o  2: 09c044d2cb43 'd'
  |
  o  1: fc055c3b4d33 'b'
  |
  o  0: b173517d0057 'a'
  
  $ chmod +x a
  $ hg commit -m "change a's flags"
  $ hg up 0
  1 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ hg rebase -r 4 -d .
  rebasing 4:0666f6a71f74 "change a's flags" (tip)
  saved backup bundle to $TESTTMP/repo1/repo3/.hg/strip-backup/0666f6a71f74-a2618702-rebase.hg
  $ hg up -q tip
  $ ls -l a | cut -c -10
  -rwxr-xr-x
  $ cd ..

Rebase the working copy parent:
  $ cd repo2
  $ hg up -C 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase -r 3 -d 0 --debug | egrep 'rebasing|disabling'
  rebasing in-memory
  rebasing 3:753feb6fd12a "c" (tip)
  $ tglog
  @  3: 844a7de3e617 'c'
  |
  | o  2: 09c044d2cb43 'd'
  | |
  | o  1: fc055c3b4d33 'b'
  |/
  o  0: b173517d0057 'a'
  
Rerun with merge conflicts, demonstrating switching to on-disk merge:
  $ hg up 2
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo 'e' > c
  $ hg add
  adding c
  $ hg com -m 'e -> c'
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ tglog
  o  4: 6af061510c70 'e -> c'
  |
  | o  3: 844a7de3e617 'c'
  | |
  o |  2: 09c044d2cb43 'd'
  | |
  @ |  1: fc055c3b4d33 'b'
  |/
  o  0: b173517d0057 'a'
  
  $ hg rebase -r 3 -d 4
  rebasing 3:844a7de3e617 "c"
  merging c
  hit merge conflicts (in c); switching to on-disk merge
  rebasing 3:844a7de3e617 "c"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted

Allow the working copy parent to be rebased with IMM:
  $ setconfig rebase.experimental.inmemorywarning='rebasing in-memory!'
  $ hg up -qC 3
  $ hg rebase -r . -d 2
  rebasing in-memory!
  rebasing 3:844a7de3e617 "c"
  saved backup bundle to $TESTTMP/repo1/repo2/.hg/strip-backup/844a7de3e617-108d0332-rebase.hg
  $ tglog
  @  4: 6f55b7035492 'c'
  |
  | o  3: 6af061510c70 'e -> c'
  |/
  o  2: 09c044d2cb43 'd'
  |
  o  1: fc055c3b4d33 'b'
  |
  o  0: b173517d0057 'a'
  

Ensure if we rebase the WCP, we still require the working copy to be clean up
front:
  $ echo 'd' > i
  $ hg add i
  $ hg rebase -r . -d 0
  abort: uncommitted changes
  [255]
  $ hg up -Cq .
  $ hg st
  ? c.orig
  ? i
