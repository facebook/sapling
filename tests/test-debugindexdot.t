Just exercize debugindexdot
Create a short file history including a merge.
  $ hg init t
  $ cd t
  $ echo a > a
  $ hg ci -qAm t1 -d '0 0'
  $ echo a >> a
  $ hg ci -m t2 -d '1 0'
  $ hg up -qC 0
  $ echo b >> a
  $ hg ci -m t3 -d '2 0'
  created new head
  $ HGMERGE=true hg merge -q
  $ hg ci -m merge -d '3 0'

  $ hg debugindexdot .hg/store/data/a.i
  digraph G {
  	-1 -> 0
  	0 -> 1
  	0 -> 2
  	2 -> 3
  	1 -> 3
  }
