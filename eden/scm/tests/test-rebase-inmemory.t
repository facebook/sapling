#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

#require symlink execbit

  $ enable amend morestatus rebase
  $ setconfig morestatus.show=True
  $ setconfig diff.git=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True

Rebase a simple DAG:
  $ hg init repo1
  $ cd repo1
  $ drawdag <<'EOS'
  > c b
  > |/
  > d
  > |
  > a
  > EOS
  $ hg up -C $a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglog
  o  814f6bd05178 'c'
  │
  │ o  db0e82a16a62 'b'
  ├─╯
  o  02952614a83d 'd'
  │
  @  b173517d0057 'a'
  
  $ hg cat -r $c c
  c (no-eol)
  $ hg cat -r $b b
  b (no-eol)
  $ hg rebase --debug -r $b -d $c 2>&1 | grep rebasing
  rebasing in-memory
  rebasing db0e82a16a62 "b"
  $ tglog
  o  ca58782ad1e4 'b'
  │
  o  814f6bd05178 'c'
  │
  o  02952614a83d 'd'
  │
  @  b173517d0057 'a'
  
  $ hg cat -r $b b
  b (no-eol)
  $ hg cat -r $c c
  c (no-eol)

Case 2:
  $ hg init repo2
  $ cd repo2
  $ drawdag <<'EOS'
  > c b
  > |/
  > d
  > |
  > a
  > EOS

Add a symlink and executable file:
  $ hg up -C $c
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ln -s somefile e
  $ echo f > f
  $ chmod +x f
  $ hg add e f
  $ hg amend -q
  $ hg up -Cq $a

Write files to the working copy, and ensure they're still there after the rebase
  $ echo "abc" > a
  $ ln -s def b
  $ echo "ghi" > c
  $ echo "jkl" > d
  $ echo "mno" > e
  $ tglog
  o  f56b71190a8f 'c'
  │
  │ o  db0e82a16a62 'b'
  ├─╯
  o  02952614a83d 'd'
  │
  @  b173517d0057 'a'
  
  $ hg cat -r 'desc(c)' c
  c (no-eol)
  $ hg cat -r 'desc(b)' b
  b (no-eol)
  $ hg cat -r 'desc(c)' e
  somefile (no-eol)
  $ hg rebase --debug -s $b -d $a 2>&1 | grep rebasing
  rebasing in-memory
  rebasing db0e82a16a62 "b"
  $ tglog
  o  fc055c3b4d33 'b'
  │
  │ o  f56b71190a8f 'c'
  │ │
  │ o  02952614a83d 'd'
  ├─╯
  @  b173517d0057 'a'
  
  $ hg cat -r 'desc(c)' c
  c (no-eol)
  $ hg cat -r 'desc(b)' b
  b (no-eol)
  $ hg rebase --debug -s 'desc(d)' -d 'desc(b)' 2>&1 | grep rebasing
  rebasing in-memory
  rebasing 02952614a83d "d"
  rebasing f56b71190a8f "c"
  $ tglog
  o  753feb6fd12a 'c'
  │
  o  09c044d2cb43 'd'
  │
  o  fc055c3b4d33 'b'
  │
  @  b173517d0057 'a'
  
Ensure working copy files are still there:
  $ cat a
  abc
  $ f b
  b -> def
  $ cat e
  mno

Ensure symlink and executable files were rebased properly:
  $ hg up -Cq 'desc(c)'
  $ f e
  e -> somefile
  $ f -m f
  f: mode=755
  $ cd ..

Make a change that only changes the flags of a file and ensure it rebases
cleanly.
  $ hg clone repo2 repo3
  updating to branch default
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo3
  $ tglog
  @  753feb6fd12a 'c'
  │
  o  09c044d2cb43 'd'
  │
  o  fc055c3b4d33 'b'
  │
  o  b173517d0057 'a'
  
  $ chmod +x a
  $ hg commit -m "change a's flags"
  $ hg up 'desc(a)-desc(change)'
  1 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ hg rebase -r 'desc(change)' -d .
  rebasing 0666f6a71f74 "change a's flags"
  $ hg up -q tip
  $ f -m a
  a: mode=755
  $ cd ..

Rebase the working copy parent:
  $ cd repo2
  $ hg up -C 'desc(c)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase -r '753feb6fd12a' -d 'desc(a)' --debug 2>&1 | egrep 'rebasing|disabling'
  rebasing in-memory
  rebasing 753feb6fd12a "c"
  $ tglog
  @  844a7de3e617 'c'
  │
  │ o  09c044d2cb43 'd'
  │ │
  │ o  fc055c3b4d33 'b'
  ├─╯
  o  b173517d0057 'a'
  
Rerun with merge conflicts, demonstrating switching to on-disk merge:
  $ hg up 'desc(d)'
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo 'e' > c
  $ hg add
  adding c
  $ hg com -m 'e -> c'
  $ hg up 'desc(b)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ tglog
  o  6af061510c70 'e -> c'
  │
  │ o  844a7de3e617 'c'
  │ │
  o │  09c044d2cb43 'd'
  │ │
  @ │  fc055c3b4d33 'b'
  ├─╯
  o  b173517d0057 'a'
  
  $ hg rebase -r 844a7de3e617 -d 'desc(e)'
  rebasing 844a7de3e617 "c"
  merging c
  hit merge conflicts (in c); switching to on-disk merge
  rebasing 844a7de3e617 "c"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted

Allow the working copy parent to be rebased with IMM:
  $ setconfig rebase.experimental.inmemorywarning='rebasing in-memory!'
  $ hg up -qC 'desc(c)-desc(e)'
  $ hg rebase -r . -d 'desc(d)'
  rebasing in-memory!
  rebasing 844a7de3e617 "c"
  $ tglog
  @  6f55b7035492 'c'
  │
  │ o  6af061510c70 'e -> c'
  ├─╯
  o  09c044d2cb43 'd'
  │
  o  fc055c3b4d33 'b'
  │
  o  b173517d0057 'a'
  

Ensure if we rebase the WCP, we still require the working copy to be clean up
front:
  $ echo 'd' > i
  $ hg add i
  $ hg rebase -r . -d 'desc(a)'
  abort: uncommitted changes
  [255]
  $ hg up -Cq .
  $ hg st
  ? c.orig
  ? i
