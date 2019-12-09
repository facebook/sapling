#chg-compatible

  $ setconfig extensions.treemanifest=!
Test log FILE history handling with renames / file node collisions.

  $ . $TESTDIR/library.sh

Create a repo with two files X and Y. Create 3 branches (B+E, C, D) where X and
Y are swapped 0 to 2 times, and merge those branches.

  $ newrepo nonshallow1
  $ setconfig remotefilelog.server=1
  $ drawdag <<'EOS'
  >   H
  >  / \
  > F   G
  > |\ /|
  > E | |   # E/X=2 (copied from Y)
  > | | |   # E/Y=2 (copied from X)
  > | | |
  > | | D   # D/X=2
  > | | |   # D/Y=2
  > | | |
  > | C |   # C/X=2 (copied from Y)
  > | | |   # C/Y=2 (copied from X)
  > | | |
  > B | |   # B/X=2 (copied from Y)
  > | | |   # B/Y=2 (copied from X)
  >  \|/
  >   A     # A/X=1
  >         # A/Y=1
  >         # drawdag.defaultfiles=false
  > EOS

  $ for i in B C D E H; do
  >   echo log via $i:
  >   hg log -fr "desc($i)" X -T '{desc}\n' -G
  > done
  log via B:
  o  B
  |
  o  A
  
  log via C:
  o  A
  
  log via D:
  o  D
  |
  o  A
  
  log via E:
  o  E
  |
  o  B
  |
  o  A
  
  log via H:
  o  E
  |
  | o  D
  | |
  o |  B
  |/
  o  A
  
(incorrect: C disappeared in "log via H" and "log via C")

"--removed" does not change things.

  $ hg log -fr "desc(H)" X -T '{desc}\n' -G
  o  E
  |
  | o  D
  | |
  o |  B
  |/
  o  A
  

Try the same on a repacked shallow repo

  $ hgcloneshallow ssh://user@dummy/nonshallow1 $TESTTMP/shallow1 -q
  * files fetched over * (glob)
  $ cd $TESTTMP/shallow1

  $ hg repack

  $ for i in B C D E H; do
  >   echo log via $i:
  >   hg log -fr "desc($i)" X -T '{desc}\n' -G
  > done
  log via B:
  o  B
  |
  o  A
  
  log via C:
  o  C
  |
  o  A
  
  log via D:
  o  D
  |
  o  A
  
  log via E:
  o  E
  |
  o  B
  |
  o  A
  
  log via H:
  o  E
  |
  | o  D
  | |
  | | o  C
  | |/
  o |  B
  |/
  o  A
  

Test file node collisions created by file delection.

Create a repo with one file X. Delete and recreate a few times.

  $ newrepo nonshallow2
  $ setconfig remotefilelog.server=1
  $ drawdag <<'EOS'
  > G       # G/X=
  > |
  > F       # F/X=(deleted)
  > |\
  > C E     # C/X=
  > | |     # E/X=
  > B D     # B/X=(deleted)
  > |/      # D/X=(deleted)
  > A       # A/X=
  >         # drawdag.defaultfiles=false
  > EOS

  $ for i in A C E G; do
  >   echo log via $i:
  >   hg log -fr "desc($i)" X -T '{desc}\n' -G
  > done
  log via A:
  o  A
  
  log via C:
  o  C
  |
  o  B
  |
  o  A
  
  log via E:
  o  A
  
  log via G:
  o    G
  :\
  : o  C
  : |
  : o  B
  :/
  o  A
  
(incorrect: D, E disappeared in "log via E" and "log via G"; F disappeared in "log via G")

With "--removed", it is slightly better.

  $ hg log -fr "desc(G)" X -T '{desc}\n' -G --removed
  o    G
  :\
  : o  C
  : |
  o |  D
  | |
  | o  B
  |/
  o  A
  

Try again in a shallow repo:

  $ hgcloneshallow ssh://user@dummy/nonshallow2 $TESTTMP/shallow2 -q
  * files fetched over * (glob)
  $ cd $TESTTMP/shallow2

  $ for i in A C E G; do
  >   echo log via $i:
  >   hg log -fr "desc($i)" X -T '{desc}\n' -G
  > done
  log via A:
  o  A
  
  log via C:
  o  C
  |
  o  B
  |
  o  A
  
  log via E:
  o  D
  |
  o  A
  
  log via G:
  @    G
  :\
  : o  C
  : |
  o |  D
  | |
  | o  B
  |/
  o  A
  
(incorrect: E disappeared in "log via E" and "log via G"; F disappeared in "log via G")

"--removed" does not change things.

  $ hg log -fr "desc(G)" X -T '{desc}\n' -G --removed
  @    G
  :\
  : o  C
  : |
  o |  D
  | |
  | o  B
  |/
  o  A
  
