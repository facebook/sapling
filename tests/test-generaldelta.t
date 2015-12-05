Check whether size of generaldelta revlog is not bigger than its
regular equivalent. Test would fail if generaldelta was naive
implementation of parentdelta: third manifest revision would be fully
inserted due to big distance from its paren revision (zero).

  $ hg init repo --config format.generaldelta=no --config format.usegeneraldelta=no
  $ cd repo
  $ echo foo > foo
  $ echo bar > bar
  $ echo baz > baz
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
  $ hg init client --config format.generaldelta=false --config format.usegeneraldelta=false
  $ cd client
  $ hg pull -q ../server -r 4
  $ hg debugindex x
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       1 1406e7411862 000000000000 000000000000

  $ cd ..

Test "usegeneraldelta" config
(repo are general delta, but incoming bundle are not re-deltified)

delta coming from the server base delta server are not recompressed.
(also include the aggressive version for comparison)

  $ hg clone repo --pull --config format.usegeneraldelta=1 usegd
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 6 changes to 3 files (+2 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone repo --pull --config format.generaldelta=1 full
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 6 changes to 3 files (+2 heads)
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R repo debugindex -m
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     104      0       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      0       2 134fdc6fd680 cef96823c800 000000000000
       3       218     104      3       3 723508934dad cef96823c800 000000000000
  $ hg -R usegd debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     104     -1       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      1       2 134fdc6fd680 cef96823c800 000000000000
       3       218      57      0       3 723508934dad cef96823c800 000000000000
  $ hg -R full debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     104     -1       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      0       2 134fdc6fd680 cef96823c800 000000000000
       3       218      57      0       3 723508934dad cef96823c800 000000000000

Test format.aggressivemergedeltas

  $ hg init --config format.generaldelta=1 aggressive
  $ cd aggressive
  $ cat << EOF >> .hg/hgrc
  > [format]
  > generaldelta = 1
  > EOF
  $ touch a b c d e
  $ hg commit -Aqm side1
  $ hg up -q null
  $ touch x y
  $ hg commit -Aqm side2

- Verify non-aggressive merge uses p1 (commit 1) as delta parent
  $ hg merge -q 0
  $ hg commit -q -m merge
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      59     -1       0 8dde941edb6e 000000000000 000000000000
       1        59      61      0       1 315c023f341d 000000000000 000000000000
       2       120      65      1       2 2ab389a983eb 315c023f341d 8dde941edb6e

  $ hg strip -q -r . --config extensions.strip=

- Verify aggressive merge uses p2 (commit 0) as delta parent
  $ hg up -q -C 1
  $ hg merge -q 0
  $ hg commit -q -m merge --config format.aggressivemergedeltas=True
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      59     -1       0 8dde941edb6e 000000000000 000000000000
       1        59      61      0       1 315c023f341d 000000000000 000000000000
       2       120      62      0       2 2ab389a983eb 315c023f341d 8dde941edb6e

Test that strip bundle use bundle2
  $ hg --config extensions.strip= strip .
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/aggressive/.hg/strip-backup/1c5d4dc9a8b8-6c68e60c-backup.hg (glob)
  $ hg debugbundle .hg/strip-backup/*
  Stream params: {'Compression': 'BZ'}
  changegroup -- "{'version': '02'}"
      1c5d4dc9a8b8d6e1750966d343e94db665e7a1e9

  $ cd ..
