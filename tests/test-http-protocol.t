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

  $ hg serve -R server -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS

compression formats are advertised in compression capability

#if zstd
  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep '^compression=zstd,zlib$' > /dev/null
#else
  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep '^compression=zlib$' > /dev/null
#endif

  $ killdaemons.py

server.compressionengines can replace engines list wholesale

  $ hg serve --config server.compressionengines=none -R server -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS
  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep '^compression=none$' > /dev/null

  $ killdaemons.py

Order of engines can also change

  $ hg serve --config server.compressionengines=none,zlib -R server -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS
  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=capabilities' | tr ' ' '\n' | grep '^compression=none,zlib$' > /dev/null

  $ killdaemons.py

Start a default server again

  $ hg serve -R server -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS

Server should send application/mercurial-0.1 to clients if no Accept is used

  $ get-with-headers.py --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' -
  200 Script output follows
  content-type: application/mercurial-0.1
  date: * (glob)
  server: * (glob)
  transfer-encoding: chunked

Server should send application/mercurial-0.1 when client says it wants it

  $ get-with-headers.py --hgproto '0.1' --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' -
  200 Script output follows
  content-type: application/mercurial-0.1
  date: * (glob)
  server: * (glob)
  transfer-encoding: chunked

Server should send application/mercurial-0.2 when client says it wants it

  $ get-with-headers.py --hgproto '0.2' --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' -
  200 Script output follows
  content-type: application/mercurial-0.2
  date: * (glob)
  server: * (glob)
  transfer-encoding: chunked

  $ get-with-headers.py --hgproto '0.1 0.2' --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' -
  200 Script output follows
  content-type: application/mercurial-0.2
  date: * (glob)
  server: * (glob)
  transfer-encoding: chunked

Requesting a compression format that server doesn't support results will fall back to 0.1

  $ get-with-headers.py --hgproto '0.2 comp=aa' --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' -
  200 Script output follows
  content-type: application/mercurial-0.1
  date: * (glob)
  server: * (glob)
  transfer-encoding: chunked

#if zstd
zstd is used if available

  $ get-with-headers.py --hgproto '0.2 comp=zstd' $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' > resp
  $ f --size --hexdump --bytes 36 --sha1 resp
  resp: size=248, sha1=4d8d8f87fb82bd542ce52881fdc94f850748
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 04 7a 73 74 64 |t follows...zstd|
  0020: 28 b5 2f fd                                     |(./.|

#endif

application/mercurial-0.2 is not yet used on non-streaming responses

  $ get-with-headers.py --hgproto '0.2' $LOCALIP:$HGPORT '?cmd=heads' -
  200 Script output follows
  content-length: 41
  content-type: application/mercurial-0.1
  date: * (glob)
  server: * (glob)
  
  e93700bd72895c5addab234c56d4024b487a362f

Now test protocol preference usage

  $ killdaemons.py
  $ hg serve --config server.compressionengines=none,zlib -R server -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid > $DAEMON_PIDS

No Accept will send 0.1+zlib, even though "none" is preferred b/c "none" isn't supported on 0.1

  $ get-with-headers.py --headeronly $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000' Content-Type
  200 Script output follows
  content-type: application/mercurial-0.1

  $ get-with-headers.py $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000'  > resp
  $ f --size --hexdump --bytes 28 --sha1 resp
  resp: size=227, sha1=35a4c074da74f32f5440da3cbf04
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 78             |t follows..x|

Explicit 0.1 will send zlib because "none" isn't supported on 0.1

  $ get-with-headers.py --hgproto '0.1' $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000'  > resp
  $ f --size --hexdump --bytes 28 --sha1 resp
  resp: size=227, sha1=35a4c074da74f32f5440da3cbf04
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 78             |t follows..x|

0.2 with no compression will get "none" because that is server's preference
(spec says ZL and UN are implicitly supported)

  $ get-with-headers.py --hgproto '0.2' $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000'  > resp
  $ f --size --hexdump --bytes 32 --sha1 resp
  resp: size=432, sha1=ac931b412ec185a02e0e5bcff98dac83
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 04 6e 6f 6e 65 |t follows...none|

Client receives server preference even if local order doesn't match

  $ get-with-headers.py --hgproto '0.2 comp=zlib,none' $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000'  > resp
  $ f --size --hexdump --bytes 32 --sha1 resp
  resp: size=432, sha1=ac931b412ec185a02e0e5bcff98dac83
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 04 6e 6f 6e 65 |t follows...none|

Client receives only supported format even if not server preferred format

  $ get-with-headers.py --hgproto '0.2 comp=zlib' $LOCALIP:$HGPORT '?cmd=getbundle&heads=e93700bd72895c5addab234c56d4024b487a362f&common=0000000000000000000000000000000000000000'  > resp
  $ f --size --hexdump --bytes 33 --sha1 resp
  resp: size=232, sha1=a1c727f0c9693ca15742a75c30419bc36
  0000: 32 30 30 20 53 63 72 69 70 74 20 6f 75 74 70 75 |200 Script outpu|
  0010: 74 20 66 6f 6c 6c 6f 77 73 0a 0a 04 7a 6c 69 62 |t follows...zlib|
  0020: 78                                              |x|
