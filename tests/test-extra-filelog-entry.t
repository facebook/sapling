Issue351: mq: qrefresh can create extra revlog entry

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init
  $ hg qinit

  $ echo b > b
  $ hg ci -A -m foo
  adding b

  $ echo cc > b
  $ hg qnew -f foo.diff
  $ echo b > b
  $ hg qrefresh

  $ hg debugindex .hg/store/data/b.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       0 1e88685f5dde 000000000000 000000000000

