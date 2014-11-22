Check whether size of generaldelta revlog is not bigger than its
regular equivalent. Test would fail if generaldelta was naive
implementation of parentdelta: third manifest revision would be fully
inserted due to big distance from its paren revision (zero).

  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ echo bar > bar
  $ hg commit -q -Am boo
  $ hg clone --pull . ../gdrepo -q --config format.generaldelta=yes
  $ for r in 1 2 3; do
  >   echo $r > foo
  >   hg commit -q -m $r
  >   hg up -q -r 0
  >   hg pull . -q -r $r -R ../gdrepo
  > done

  $ cd ..
  >>> import os
  >>> regsize = os.stat("repo/.hg/store/00manifest.i").st_size
  >>> gdsize = os.stat("gdrepo/.hg/store/00manifest.i").st_size
  >>> if regsize < gdsize:
  ...     print 'generaldata increased size of manifest'

Verify rev reordering doesnt create invalid bundles (issue4462)
This requires a commit tree that when pulled will reorder manifest revs such
that the second manifest to create a file rev will be ordered before the first
manifest to create that file rev. We also need to do a partial pull to ensure
reordering happens. At the end we verify the linkrev points at the earliest
commit.

  $ hg init server --config format.generaldelta=True
  $ cd server
  $ touch a
  $ hg commit -Aqm a
  $ echo x > x
  $ echo y > y
  $ hg commit -Aqm xy
  $ hg up -q '.^'
  $ echo x > x
  $ echo z > z
  $ hg commit -Aqm xz
  $ hg up -q 1
  $ echo b > b
  $ hg commit -Aqm b
  $ hg merge -q 2
  $ hg commit -Aqm merge
  $ echo c > c
  $ hg commit -Aqm c
  $ hg log -G -T '{rev} {shortest(node)} {desc}'
  @  5 ebb8 c
  |
  o    4 baf7 merge
  |\
  | o  3 a129 b
  | |
  o |  2 958c xz
  | |
  | o  1 f00c xy
  |/
  o  0 3903 a
  
  $ cd ..
  $ hg init client
  $ cd client
  $ hg pull -q ../server -r 4
  $ hg debugindex x
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       1 1406e7411862 000000000000 000000000000

