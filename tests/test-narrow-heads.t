'narrow-heads' requires remotenames and visibility

  $ enable remotenames amend
  $ setconfig experimental.narrow-heads=true visibility.enabled=true mutation.record=true mutation.enabled=true mutation.date="0 0" experimental.evolution= remotenames.rename.default=remote
  $ shorttraceback

Prepare the server repo

  $ newrepo server
  $ setconfig treemanifest.server=true
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg bookmark -r $B master
  $ hg bookmark -r $A stable

Prepare the client repo
('--pull' during clone is required to get visibility requirement set)

  $ hg clone $TESTTMP/server $TESTTMP/client -q --pull
  $ cd $TESTTMP/client

Verify the commits

  $ hg bookmarks --remote
     remote/master             1:112478962961
     remote/stable             0:426bada5c675

Revsets after the initial clone

  $ hg log -Gr 'all()' -T '{desc} {remotenames} {phase}'
  @  B remote/master public
  |
  o  A remote/stable public
  
  $ hg log -Gr 'head()' -T '{desc} {remotenames}'
  @  B remote/master
  |
  ~

Make some client-side commits based on A

  $ drawdag << 'EOS'
  > D
  > |
  > C
  > |
  > A
  > EOS
  $ hg up -r $D -q
  $ hg up -r $C -q
  $ hg metaedit -m C2

Revsets after the local edits

head() should include one 'D' commit, and one 'B'
(BUG: 'B' should not be 'x')

  $ hg log -Gr 'head()' -T '{desc}'
  o  D
  |
  ~
  x  B
  |
  ~

all() should not show C
Commits under ::master should be public
(BUG: 'B' should be public! 'C' should be hidden)

  $ hg log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  o  D draft
  |
  @  C2 draft
  |
  | x  D draft
  | |
  | x  C draft
  |/
  | x  B draft remote/master
  |/
  o  A draft remote/stable
  
draft() should not show C
(BUG: neither 'B' nor 'C' should be shown)

  $ hg log -Gr 'draft()' -T '{desc}'
  o  D
  |
  @  C2
  |
  | x  D
  | |
  | x  C
  |/
  | x  B
  |/
  o  A
  
not public() should not show C
(BUG: neither 'B' nor 'C' should be shown)

  $ hg log -Gr 'not public()' -T '{desc}'
  o  D
  |
  @  C2
  |
  | x  D
  | |
  | x  C
  |/
  | x  B
  |/
  o  A
  
A:: should not show C
(BUG: 'B' should be public! 'C' should be hidden)

  $ hg log -Gr "$A::" -T '{desc}'
  o  D
  |
  @  C2
  |
  | x  D
  | |
  | x  C
  |/
  | x  B
  |/
  o  A
  
children(A) should not show C
(BUG: 'B' should be public! 'C' should be hidden)

  $ hg log -Gr "children($A)" -T '{desc}'
  @  C2
  |
  ~
  x  C
  |
  ~
  x  B
  |
  ~

predecessors(C2) should include C

  $ hg log -Gr "predecessors(desc('C2'))" -T '{desc}'
  @  C2
  |
  ~
  x  C
  |
  ~

Using commit hash to access C should be allowed

  $ hg log -r $C -T '{desc}'
  C (no-eol)
