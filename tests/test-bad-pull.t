#require serve killdaemons

#if windows
  $ hg clone http://localhost:$HGPORT/ copy
  abort: * (glob)
  [255]
#else
  $ hg clone http://localhost:$HGPORT/ copy
  abort: error: Connection refused
  [255]
#endif

  $ test -d copy
  [1]

  $ python "$TESTDIR/dumbhttp.py" -p $HGPORT --pid dumb.pid
  $ cat dumb.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/foo copy2
  abort: HTTP Error 404: * (glob)
  [255]
  $ killdaemons.py
