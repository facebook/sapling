#require serve killdaemons

  $ hg clone http://localhost:$HGPORT/ copy
  abort: * (glob)
  [255]

  $ test -d copy
  [1]

  $ hg debugpython -- "$TESTDIR/dumbhttp.py" -p $HGPORT --pid dumb.pid
  $ cat dumb.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/foo copy2
  abort: HTTP Error 404: * (glob)
  [255]
  $ killdaemons.py
