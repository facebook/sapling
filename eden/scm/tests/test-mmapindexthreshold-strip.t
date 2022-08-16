#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ setconfig experimental.changegroup3=False
  $ enable rebase
  $ setconfig experimental.mmapindexthreshold=1

  $ hg init
  $ hg debugdrawdag <<'EOS'
  > a1    # a1/a = a1
  > |
  > a     # a/a = a
  > EOS

  $ hg debugindex -c
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      55      0       0 b173517d0057 000000000000 000000000000
       1        55      59      1       1 18d792233a72 b173517d0057 000000000000
  $ hg debugstrip -r a1 -q
  $ hg debugindex -c
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      55      0       0 b173517d0057 000000000000 000000000000
