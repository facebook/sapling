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

  $ hg debugindex b
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       0 1e88685f5dde 000000000000 000000000000 (re)

