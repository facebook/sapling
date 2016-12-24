  $ cat >> $HGRCPATH << EOF
  > [web]
  > push_ssl = false
  > allow_push = *
  > EOF

  $ hg init server
  $ cd server
  $ touch a
  $ hg -q commit -A -m initial
  $ cd ..

  $ hg -R server serve -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS

compression formats are advertised in compression capability

#if zstd
  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep compression
  compression=zstd,zlib
#else
  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep compression
  compression=ZL
#endif

  $ killdaemons.py

server.compressionengines can replace engines list wholesale

  $ hg --config server.compressionengines=none -R server serve -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS
  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep compression
  compression=none

  $ killdaemons.py

Order of engines can also change

  $ hg --config server.compressionengines=none,zlib -R server serve -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS
  $ get-with-headers.py 127.0.0.1:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep compression
  compression=none,zlib

  $ killdaemons.py
