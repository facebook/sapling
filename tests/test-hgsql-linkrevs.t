  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

# Create initial repo that can be pulled out of order

  $ initclient client
  $ cd client
  $ touch 0
  $ hg commit -qAm 0
  $ hg up -q null
  $ touch 1
  $ hg commit -qAm 1
  $ hg up -q null
  $ touch 0
  $ hg commit -qAm 2
  $ hg up -q null
  $ touch 1
  $ hg commit -qAm 3
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 a84de0447720 000000000000 000000000000
       1        44      44     -1       1 eff23848989b 000000000000 000000000000
  $ cd ..

# Verify pulling out of order filelog linkrevs get reordered.
# (a normal mercurial pull here would result in order 1->0 instead of 0->1)

  $ initserver master masterrepo
  $ cd master
  $ hg pull -q -r -2 -r -3 ../client
  $ hg log --template 'rev: {rev} desc: {desc}\n'
  rev: 1 desc: 2
  rev: 0 desc: 1
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 eff23848989b 000000000000 000000000000
       1        44      44     -1       1 a84de0447720 000000000000 000000000000
  $ cd ..
