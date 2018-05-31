  $ newrepo
  $ setconfig repogenerator.filenamedircount=2
  $ setconfig repogenerator.filenameleaflength=1
  $ setconfig repogenerator.numcommits=3
  $ hg repogenerator --seed 1 --config extensions.repogenerator=
  starting commit is: -1 (goal is 2)
  created *, * sec elapsed (* commits/sec, * per hour, * per day) (glob)
  $ hg log -G -r ::tip
  o  changeset:   2:2f0eabc6bc3d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     memory commit
  |
  o  changeset:   1:272777df88de
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     memory commit
  |
  o  changeset:   0:8023a25712fb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     memory commit
  
  $ hg up -C tip
  13 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r tip -T'{files}'
  f/t/o h/w/l j/h/p j/o/e x/l/j x/z/b (no-eol)
  $ ls */*/*
  f/t/o
  h/u/h
  h/w/l
  j/h/p
  j/o/e
  l/c/h
  r/f/c
  u/y/a
  v/c/c
  w/k/a
  x/l/j
  x/z/b
  y/d/e

Set startcommit=0 and confirm it creates a commit off of 0.
  $ setconfig repogenerator.startcommit=0
  $ hg repogenerator --seed 1 --config extensions.repogenerator= -n 1
  starting commit is: 0 (goal is 2)
  created 0, * sec elapsed (* commits/sec, * per hour, * per day) (glob)
  generated 1 commits; quitting
  $ hg log -r tip~1+tip -T '{rev} '
  0 3  (no-eol)
