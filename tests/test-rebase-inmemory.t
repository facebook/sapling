#require symlink execbit
  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > amend=
  > rebase=
  > [rebase]
  > experimental.inmemory=1
  > [diff]
  > git=1
  > [alias]
  > tglog = log -G --template "{rev}: {node|short} '{desc}'\n"
  > EOF

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
  $ hg tglog
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
  $ hg tglog
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
  $ hg tglog
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
  $ hg tglog
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
  $ hg tglog
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

Rebase the working copy parent, which should default to an on-disk merge even if
we requested in-memory.
  $ hg up -C 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase -r 3 -d 0 --debug | grep rebasing
  rebasing on disk
  rebasing 3:753feb6fd12a "c" (tip)
  $ hg tglog
  @  3: 844a7de3e617 'c'
  |
  | o  2: 09c044d2cb43 'd'
  | |
  | o  1: fc055c3b4d33 'b'
  |/
  o  0: b173517d0057 'a'
  
Rerun with merge conflicts:
  $ hg up 2
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo 'e' > c
  $ hg add
  adding c
  $ hg com -m 'e -> c'
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg tglog
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
  hit merge conflicts; re-running rebase without in-memory merge (in-memory merge does not support merge conflicts)
  rebase aborted
  rebasing 3:844a7de3e617 "c"
  merging c
  warning: conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Also ensure the last test works if rebasing the WCP (turning off IMM mid-call):
  $ hg rebase --abort
  rebase aborted
  $ hg up -C 3
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg rebase -r 3 -d 4
  rebasing 3:844a7de3e617 "c"
  merging c
  warning: conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
