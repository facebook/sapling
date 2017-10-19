#require killdaemons serve zstd

Client version is embedded in HTTP request and is effectively dynamic. Pin the
version so behavior is deterministic.

  $ cat > fakeversion.py << EOF
  > from mercurial import util
  > util.version = lambda: '4.2'
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fakeversion = `pwd`/fakeversion.py
  > [devel]
  > legacy.exchange = phases
  > EOF

  $ hg init server0
  $ cd server0
  $ touch foo
  $ hg -q commit -A -m initial

Also disable compression because zstd is optional and causes output to vary
and because debugging partial responses is hard when compression is involved

  $ cat > .hg/hgrc << EOF
  > [extensions]
  > badserver = $TESTDIR/badserverext.py
  > [server]
  > compressionengines = none
  > EOF

Failure to accept() socket should result in connection related error message

  $ hg serve --config badserver.closebeforeaccept=true -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: Connection reset by peer (no-windows !)
  abort: error: An existing connection was forcibly closed by the remote host (windows !)
  [255]

(The server exits on its own, but there is a race between that and starting a new server.
So ensure the process is dead.)

  $ killdaemons.py $DAEMON_PIDS

Failure immediately after accept() should yield connection related error message

  $ hg serve --config badserver.closeafteraccept=true -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid > $DAEMON_PIDS

TODO: this usually outputs good results, but sometimes emits abort:
error: '' on FreeBSD and OS X.
What we ideally want are:

abort: error: Connection reset by peer (no-windows !)
abort: error: An existing connection was forcibly closed by the remote host (windows !)

The flakiness in this output was observable easily with
--runs-per-test=20 on macOS 10.12 during the freeze for 4.2.
  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: * (glob)
  [255]

  $ killdaemons.py $DAEMON_PIDS

Failure to read all bytes in initial HTTP request should yield connection related error message

  $ hg serve --config badserver.closeafterrecvbytes=1 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: bad HTTP status line: ''
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(1 from 65537) -> (1) G
  read limit reached; closing socket

  $ rm -f error.log

Same failure, but server reads full HTTP request line

  $ hg serve --config badserver.closeafterrecvbytes=40 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: bad HTTP status line: ''
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(40 from 65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(7 from -1) -> (7) Accept-
  read limit reached; closing socket

  $ rm -f error.log

Failure on subsequent HTTP request on the same socket (cmd?batch)

  $ hg serve --config badserver.closeafterrecvbytes=210 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: bad HTTP status line: ''
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(210 from 65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(177 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(150 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(115 from -1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(9? from -1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n (glob)
  readline(4? from -1) -> (2) \r\n (glob)
  write(36) -> HTTP/1.1 200 Script output follows\r\n
  write(23) -> Server: badhttpserver\r\n
  write(37) -> Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41) -> Content-Type: application/mercurial-0.1\r\n
  write(21) -> Content-Length: 405\r\n
  write(2) -> \r\n
  write(405) -> lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(4? from 65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n (glob)
  readline(1? from -1) -> (1?) Accept-Encoding* (glob)
  read limit reached; closing socket
  readline(210 from 65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(184 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(157 from -1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(128 from -1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(87 from -1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(39 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(4 from -1) -> (4) host
  read limit reached; closing socket

  $ rm -f error.log

Failure to read getbundle HTTP request

  $ hg serve --config badserver.closeafterrecvbytes=292 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: error: bad HTTP status line: ''
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(292 from 65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(259 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(232 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(197 from -1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(17? from -1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n (glob)
  readline(12? from -1) -> (2) \r\n (glob)
  write(36) -> HTTP/1.1 200 Script output follows\r\n
  write(23) -> Server: badhttpserver\r\n
  write(37) -> Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41) -> Content-Type: application/mercurial-0.1\r\n
  write(21) -> Content-Length: 405\r\n
  write(2) -> \r\n
  write(405) -> lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline\(12[34] from 65537\) -> \(2[67]\) GET /\?cmd=batch HTTP/1.1\\r\\n (re)
  readline(9? from -1) -> (27) Accept-Encoding: identity\r\n (glob)
  readline(7? from -1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n (glob)
  readline(4? from -1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n (glob)
  readline(1 from -1) -> (1) x (?)
  read limit reached; closing socket
  readline(292 from 65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(266 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(239 from -1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(210 from -1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(169 from -1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(121 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(86 from -1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(6? from -1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n (glob)
  readline(1? from -1) -> (2) \r\n (glob)
  write(36) -> HTTP/1.1 200 Script output follows\r\n
  write(23) -> Server: badhttpserver\r\n
  write(37) -> Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41) -> Content-Type: application/mercurial-0.1\r\n
  write(20) -> Content-Length: 42\r\n
  write(2) -> \r\n
  write(42) -> 96ee1d7354c4ad7372047672c36a1f561e3a6a4c\n;
  readline\(1[23] from 65537\) -> \(1[23]\) GET /\?cmd=ge.? (re)
  read limit reached; closing socket
  readline(292 from 65537) -> (30) GET /?cmd=getbundle HTTP/1.1\r\n
  readline(262 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(235 from -1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(206 from -1) -> (206) x-hgarg-1: bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Ali
  read limit reached; closing socket

  $ rm -f error.log

Now do a variation using POST to send arguments

  $ hg serve --config experimental.httppostargs=true --config badserver.closeafterrecvbytes=315 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: bad HTTP status line: ''
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(315 from 65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(282 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(255 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(220 from -1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(19? from -1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n (glob)
  readline(14? from -1) -> (2) \r\n (glob)
  write(36) -> HTTP/1.1 200 Script output follows\r\n
  write(23) -> Server: badhttpserver\r\n
  write(37) -> Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41) -> Content-Type: application/mercurial-0.1\r\n
  write(21) -> Content-Length: 418\r\n
  write(2) -> \r\n
  write(418) -> lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httppostargs httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline\(14[67] from 65537\) -> \(2[67]\) POST /\?cmd=batch HTTP/1.1\\r\\n (re)
  readline\(1(19|20) from -1\) -> \(27\) Accept-Encoding: identity\\r\\n (re)
  readline(9? from -1) -> (41) content-type: application/mercurial-0.1\r\n (glob)
  readline(5? from -1) -> (19) vary: X-HgProto-1\r\n (glob)
  readline(3? from -1) -> (19) x-hgargs-post: 28\r\n (glob)
  readline(1? from -1) -> (1?) x-hgproto-1: * (glob)
  read limit reached; closing socket
  readline(315 from 65537) -> (27) POST /?cmd=batch HTTP/1.1\r\n
  readline(288 from -1) -> (27) Accept-Encoding: identity\r\n
  readline(261 from -1) -> (41) content-type: application/mercurial-0.1\r\n
  readline(220 from -1) -> (19) vary: X-HgProto-1\r\n
  readline(201 from -1) -> (19) x-hgargs-post: 28\r\n
  readline(182 from -1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(134 from -1) -> (35) accept: application/mercurial-0.1\r\n
  readline(99 from -1) -> (20) content-length: 28\r\n
  readline(79 from -1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(5? from -1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n (glob)
  readline(? from -1) -> (2) \r\n (glob)
  read(? from 28) -> (?) cmds=* (glob)
  read limit reached, closing socket
  write(36) -> HTTP/1.1 500 Internal Server Error\r\n

  $ rm -f error.log

Now move on to partial server responses

Server sends a single character from the HTTP response line

  $ hg serve --config badserver.closeaftersendbytes=1 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  abort: error: bad HTTP status line: H
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(1 from 36) -> (0) H
  write limit reached; closing socket
  write(36) -> HTTP/1.1 500 Internal Server Error\r\n

  $ rm -f error.log

Server sends an incomplete capabilities response body

  $ hg serve --config badserver.closeaftersendbytes=180 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  abort: HTTP request error (incomplete response; expected 385 bytes got 20)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (144) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (121) Server: badhttpserver\r\n
  write(37 from 37) -> (84) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (43) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (22) Content-Length: 405\r\n
  write(2 from 2) -> (20) \r\n
  write(20 from 405) -> (0) lookup changegroupsu
  write limit reached; closing socket

  $ rm -f error.log

Server sends incomplete headers for batch request

  $ hg serve --config badserver.closeaftersendbytes=695 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

TODO this output is horrible

  $ hg clone http://localhost:$HGPORT/ clone
  abort: 'http://localhost:$HGPORT/' does not appear to be an hg repository:
  ---%<--- (application/mercuria)
  
  ---%<---
  !
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (659) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (636) Server: badhttpserver\r\n
  write(37 from 37) -> (599) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (558) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (537) Content-Length: 405\r\n
  write(2 from 2) -> (535) \r\n
  write(405 from 405) -> (130) lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (94) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (71) Server: badhttpserver\r\n
  write(37 from 37) -> (34) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(34 from 41) -> (0) Content-Type: application/mercuria
  write limit reached; closing socket
  write(36) -> HTTP/1.1 500 Internal Server Error\r\n

  $ rm -f error.log

Server sends an incomplete HTTP response body to batch request

  $ hg serve --config badserver.closeaftersendbytes=760 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

TODO client spews a stack due to uncaught ValueError in batch.results()
#if no-chg
  $ hg clone http://localhost:$HGPORT/ clone 2> /dev/null
  [1]
#else
  $ hg clone http://localhost:$HGPORT/ clone 2> /dev/null
  [255]
#endif

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (724) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (701) Server: badhttpserver\r\n
  write(37 from 37) -> (664) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (623) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (602) Content-Length: 405\r\n
  write(2 from 2) -> (600) \r\n
  write(405 from 405) -> (195) lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (159) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (136) Server: badhttpserver\r\n
  write(37 from 37) -> (99) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (58) Content-Type: application/mercurial-0.1\r\n
  write(20 from 20) -> (38) Content-Length: 42\r\n
  write(2 from 2) -> (36) \r\n
  write(36 from 42) -> (0) 96ee1d7354c4ad7372047672c36a1f561e3a
  write limit reached; closing socket

  $ rm -f error.log

Server sends incomplete headers for getbundle response

  $ hg serve --config badserver.closeaftersendbytes=895 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

TODO this output is terrible

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: 'http://localhost:$HGPORT/' does not appear to be an hg repository:
  ---%<--- (application/mercuri)
  
  ---%<---
  !
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (859) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (836) Server: badhttpserver\r\n
  write(37 from 37) -> (799) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (758) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (737) Content-Length: 405\r\n
  write(2 from 2) -> (735) \r\n
  write(405 from 405) -> (330) lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (294) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (271) Server: badhttpserver\r\n
  write(37 from 37) -> (234) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (193) Content-Type: application/mercurial-0.1\r\n
  write(20 from 20) -> (173) Content-Length: 42\r\n
  write(2 from 2) -> (171) \r\n
  write(42 from 42) -> (129) 96ee1d7354c4ad7372047672c36a1f561e3a6a4c\n;
  readline(65537) -> (30) GET /?cmd=getbundle HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (396) x-hgarg-1: bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=96ee1d7354c4ad7372047672c36a1f561e3a6a4c&listkeys=phases%2Cbookmarks\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (93) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (70) Server: badhttpserver\r\n
  write(37 from 37) -> (33) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(33 from 41) -> (0) Content-Type: application/mercuri
  write limit reached; closing socket
  write(36) -> HTTP/1.1 500 Internal Server Error\r\n

  $ rm -f error.log

Server sends empty HTTP body for getbundle

  $ hg serve --config badserver.closeaftersendbytes=933 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (897) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (874) Server: badhttpserver\r\n
  write(37 from 37) -> (837) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (796) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (775) Content-Length: 405\r\n
  write(2 from 2) -> (773) \r\n
  write(405 from 405) -> (368) lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (332) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (309) Server: badhttpserver\r\n
  write(37 from 37) -> (272) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (231) Content-Type: application/mercurial-0.1\r\n
  write(20 from 20) -> (211) Content-Length: 42\r\n
  write(2 from 2) -> (209) \r\n
  write(42 from 42) -> (167) 96ee1d7354c4ad7372047672c36a1f561e3a6a4c\n;
  readline(65537) -> (30) GET /?cmd=getbundle HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (396) x-hgarg-1: bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=96ee1d7354c4ad7372047672c36a1f561e3a6a4c&listkeys=phases%2Cbookmarks\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (131) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (108) Server: badhttpserver\r\n
  write(37 from 37) -> (71) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (30) Content-Type: application/mercurial-0.2\r\n
  write(28 from 28) -> (2) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (0) \r\n
  write limit reached; closing socket
  write(36) -> HTTP/1.1 500 Internal Server Error\r\n

  $ rm -f error.log

Server sends partial compression string

  $ hg serve --config badserver.closeaftersendbytes=945 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response; expected 1 bytes got 3)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ cat error.log
  readline(65537) -> (33) GET /?cmd=capabilities HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (909) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (886) Server: badhttpserver\r\n
  write(37 from 37) -> (849) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (808) Content-Type: application/mercurial-0.1\r\n
  write(21 from 21) -> (787) Content-Length: 405\r\n
  write(2 from 2) -> (785) \r\n
  write(405 from 405) -> (380) lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch streamreqs=generaldelta,revlogv1 bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 httpmediatype=0.1rx,0.1tx,0.2tx compression=none
  readline(65537) -> (26) GET /?cmd=batch HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (41) x-hgarg-1: cmds=heads+%3Bknown+nodes%3D\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (344) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (321) Server: badhttpserver\r\n
  write(37 from 37) -> (284) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (243) Content-Type: application/mercurial-0.1\r\n
  write(20 from 20) -> (223) Content-Length: 42\r\n
  write(2 from 2) -> (221) \r\n
  write(42 from 42) -> (179) 96ee1d7354c4ad7372047672c36a1f561e3a6a4c\n;
  readline(65537) -> (30) GET /?cmd=getbundle HTTP/1.1\r\n
  readline(-1) -> (27) Accept-Encoding: identity\r\n
  readline(-1) -> (29) vary: X-HgArg-1,X-HgProto-1\r\n
  readline(-1) -> (396) x-hgarg-1: bundlecaps=HG20%2Cbundle2%3DHG20%250Achangegroup%253D01%252C02%250Adigests%253Dmd5%252Csha1%252Csha512%250Aerror%253Dabort%252Cunsupportedcontent%252Cpushraced%252Cpushkey%250Ahgtagsfnodes%250Alistkeys%250Apushkey%250Aremote-changegroup%253Dhttp%252Chttps&cg=1&common=0000000000000000000000000000000000000000&heads=96ee1d7354c4ad7372047672c36a1f561e3a6a4c&listkeys=phases%2Cbookmarks\r\n
  readline(-1) -> (48) x-hgproto-1: 0.1 0.2 comp=zstd,zlib,none,bzip2\r\n
  readline(-1) -> (35) accept: application/mercurial-0.1\r\n
  readline(-1) -> (2?) host: localhost:$HGPORT\r\n (glob)
  readline(-1) -> (49) user-agent: mercurial/proto-1.0 (Mercurial 4.2)\r\n
  readline(-1) -> (2) \r\n
  write(36 from 36) -> (143) HTTP/1.1 200 Script output follows\r\n
  write(23 from 23) -> (120) Server: badhttpserver\r\n
  write(37 from 37) -> (83) Date: Fri, 14 Apr 2017 00:00:00 GMT\r\n
  write(41 from 41) -> (42) Content-Type: application/mercurial-0.2\r\n
  write(28 from 28) -> (14) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (12) \r\n
  write(6 from 6) -> (6) 1\\r\\n\x04\\r\\n (esc)
  write(6 from 9) -> (0) 4\r\nnon
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server sends partial bundle2 header magic

  $ hg serve --config badserver.closeaftersendbytes=954 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response; expected 1 bytes got 3)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -7 error.log
  write(28 from 28) -> (23) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (21) \r\n
  write(6 from 6) -> (15) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (6) 4\r\nnone\r\n
  write(6 from 9) -> (0) 4\r\nHG2
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server sends incomplete bundle2 stream params length

  $ hg serve --config badserver.closeaftersendbytes=963 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response; expected 1 bytes got 3)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -8 error.log
  write(28 from 28) -> (32) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (30) \r\n
  write(6 from 6) -> (24) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (15) 4\r\nnone\r\n
  write(9 from 9) -> (6) 4\r\nHG20\r\n
  write(6 from 9) -> (0) 4\\r\\n\x00\x00\x00 (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Servers stops after bundle2 stream params header

  $ hg serve --config badserver.closeaftersendbytes=966 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -8 error.log
  write(28 from 28) -> (35) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (33) \r\n
  write(6 from 6) -> (27) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (18) 4\r\nnone\r\n
  write(9 from 9) -> (9) 4\r\nHG20\r\n
  write(9 from 9) -> (0) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops sending after bundle2 part header length

  $ hg serve --config badserver.closeaftersendbytes=975 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -9 error.log
  write(28 from 28) -> (44) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (42) \r\n
  write(6 from 6) -> (36) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (27) 4\r\nnone\r\n
  write(9 from 9) -> (18) 4\r\nHG20\r\n
  write(9 from 9) -> (9) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (0) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops sending after bundle2 part header

  $ hg serve --config badserver.closeaftersendbytes=1022 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  transaction abort!
  rollback completed
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -10 error.log
  write(28 from 28) -> (91) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (89) \r\n
  write(6 from 6) -> (83) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (74) 4\r\nnone\r\n
  write(9 from 9) -> (65) 4\r\nHG20\r\n
  write(9 from 9) -> (56) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (47) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (0) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops after bundle2 part payload chunk size

  $ hg serve --config badserver.closeaftersendbytes=1031 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  transaction abort!
  rollback completed
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -11 error.log
  write(28 from 28) -> (100) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (98) \r\n
  write(6 from 6) -> (92) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (83) 4\r\nnone\r\n
  write(9 from 9) -> (74) 4\r\nHG20\r\n
  write(9 from 9) -> (65) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (56) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (9) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write(9 from 9) -> (0) 4\\r\\n\x00\x00\x01\xd2\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops sending in middle of bundle2 payload chunk

  $ hg serve --config badserver.closeaftersendbytes=1504 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  transaction abort!
  rollback completed
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -12 error.log
  write(28 from 28) -> (573) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (571) \r\n
  write(6 from 6) -> (565) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (556) 4\r\nnone\r\n
  write(9 from 9) -> (547) 4\r\nHG20\r\n
  write(9 from 9) -> (538) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (529) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (482) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write(9 from 9) -> (473) 4\\r\\n\x00\x00\x01\xd2\\r\\n (esc)
  write(473 from 473) -> (0) 1d2\\r\\n\x00\x00\x00\xb2\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00>6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50\\ntest\\n0 0\\nfoo\\n\\ninitial\x00\x00\x00\x00\x00\x00\x00\xa1j=\xf4\xde8\x8f<O\x8e(\xf4\xf9\xa8\x14)\x9a<\xbb_P\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00-foo\x00b80de5d138758541c5f05265ad144ab9fa86d1db\\n\x00\x00\x00\x00\x00\x00\x00\x07foo\x00\x00\x00h\xb8\\r\xe5\xd18u\x85A\xc5\xf0Re\xad\x14J\xb9\xfa\x86\xd1\xdb\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops sending after 0 length payload chunk size

  $ hg serve --config badserver.closeaftersendbytes=1513 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  transaction abort!
  rollback completed
  abort: HTTP request error (incomplete response)
  (this may be an intermittent network failure; if the error persists, consider contacting the network or server operator)
  [255]

  $ killdaemons.py $DAEMON_PIDS

  $ tail -13 error.log
  write(28 from 28) -> (582) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (580) \r\n
  write(6 from 6) -> (574) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (565) 4\r\nnone\r\n
  write(9 from 9) -> (556) 4\r\nHG20\r\n
  write(9 from 9) -> (547) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (538) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (491) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write(9 from 9) -> (482) 4\\r\\n\x00\x00\x01\xd2\\r\\n (esc)
  write(473 from 473) -> (9) 1d2\\r\\n\x00\x00\x00\xb2\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00>6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50\\ntest\\n0 0\\nfoo\\n\\ninitial\x00\x00\x00\x00\x00\x00\x00\xa1j=\xf4\xde8\x8f<O\x8e(\xf4\xf9\xa8\x14)\x9a<\xbb_P\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00-foo\x00b80de5d138758541c5f05265ad144ab9fa86d1db\\n\x00\x00\x00\x00\x00\x00\x00\x07foo\x00\x00\x00h\xb8\\r\xe5\xd18u\x85A\xc5\xf0Re\xad\x14J\xb9\xfa\x86\xd1\xdb\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (0) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log

Server stops sending after 0 part bundle part header (indicating end of bundle2 payload)
This is before the 0 size chunked transfer part that signals end of HTTP response.

  $ hg serve --config badserver.closeaftersendbytes=1710 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 96ee1d7354c4
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ killdaemons.py $DAEMON_PIDS

  $ tail -22 error.log
  write(28 from 28) -> (779) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (777) \r\n
  write(6 from 6) -> (771) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (762) 4\r\nnone\r\n
  write(9 from 9) -> (753) 4\r\nHG20\r\n
  write(9 from 9) -> (744) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (735) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (688) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write(9 from 9) -> (679) 4\\r\\n\x00\x00\x01\xd2\\r\\n (esc)
  write(473 from 473) -> (206) 1d2\\r\\n\x00\x00\x00\xb2\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00>6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50\\ntest\\n0 0\\nfoo\\n\\ninitial\x00\x00\x00\x00\x00\x00\x00\xa1j=\xf4\xde8\x8f<O\x8e(\xf4\xf9\xa8\x14)\x9a<\xbb_P\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00-foo\x00b80de5d138758541c5f05265ad144ab9fa86d1db\\n\x00\x00\x00\x00\x00\x00\x00\x07foo\x00\x00\x00h\xb8\\r\xe5\xd18u\x85A\xc5\xf0Re\xad\x14J\xb9\xfa\x86\xd1\xdb\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (197) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (188) 4\\r\\n\x00\x00\x00 \\r\\n (esc)
  write(38 from 38) -> (150) 20\\r\\n\x08LISTKEYS\x00\x00\x00\x01\x01\x00	\x06namespacephases\\r\\n (esc)
  write(9 from 9) -> (141) 4\\r\\n\x00\x00\x00:\\r\\n (esc)
  write(64 from 64) -> (77) 3a\r\n96ee1d7354c4ad7372047672c36a1f561e3a6a4c	1\npublishing	True\r\n
  write(9 from 9) -> (68) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (59) 4\\r\\n\x00\x00\x00#\\r\\n (esc)
  write(41 from 41) -> (18) 23\\r\\n\x08LISTKEYS\x00\x00\x00\x02\x01\x00		namespacebookmarks\\r\\n (esc)
  write(9 from 9) -> (9) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (0) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log
  $ rm -rf clone

Server sends a size 0 chunked-transfer size without terminating \r\n

  $ hg serve --config badserver.closeaftersendbytes=1713 -p $HGPORT -d --pid-file=hg.pid -E error.log
  $ cat hg.pid > $DAEMON_PIDS

  $ hg clone http://localhost:$HGPORT/ clone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 96ee1d7354c4
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ killdaemons.py $DAEMON_PIDS

  $ tail -23 error.log
  write(28 from 28) -> (782) Transfer-Encoding: chunked\r\n
  write(2 from 2) -> (780) \r\n
  write(6 from 6) -> (774) 1\\r\\n\x04\\r\\n (esc)
  write(9 from 9) -> (765) 4\r\nnone\r\n
  write(9 from 9) -> (756) 4\r\nHG20\r\n
  write(9 from 9) -> (747) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (738) 4\\r\\n\x00\x00\x00)\\r\\n (esc)
  write(47 from 47) -> (691) 29\\r\\n\x0bCHANGEGROUP\x00\x00\x00\x00\x01\x01\x07\x02	\x01version02nbchanges1\\r\\n (esc)
  write(9 from 9) -> (682) 4\\r\\n\x00\x00\x01\xd2\\r\\n (esc)
  write(473 from 473) -> (209) 1d2\\r\\n\x00\x00\x00\xb2\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00>6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50\\ntest\\n0 0\\nfoo\\n\\ninitial\x00\x00\x00\x00\x00\x00\x00\xa1j=\xf4\xde8\x8f<O\x8e(\xf4\xf9\xa8\x14)\x9a<\xbb_P\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00-foo\x00b80de5d138758541c5f05265ad144ab9fa86d1db\\n\x00\x00\x00\x00\x00\x00\x00\x07foo\x00\x00\x00h\xb8\\r\xe5\xd18u\x85A\xc5\xf0Re\xad\x14J\xb9\xfa\x86\xd1\xdb\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x96\xee\x1dsT\xc4\xadsr\x04vr\xc3j\x1fV\x1e:jL\x00\x00\x00\x00\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (200) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (191) 4\\r\\n\x00\x00\x00 \\r\\n (esc)
  write(38 from 38) -> (153) 20\\r\\n\x08LISTKEYS\x00\x00\x00\x01\x01\x00	\x06namespacephases\\r\\n (esc)
  write(9 from 9) -> (144) 4\\r\\n\x00\x00\x00:\\r\\n (esc)
  write(64 from 64) -> (80) 3a\r\n96ee1d7354c4ad7372047672c36a1f561e3a6a4c	1\npublishing	True\r\n
  write(9 from 9) -> (71) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (62) 4\\r\\n\x00\x00\x00#\\r\\n (esc)
  write(41 from 41) -> (21) 23\\r\\n\x08LISTKEYS\x00\x00\x00\x02\x01\x00		namespacebookmarks\\r\\n (esc)
  write(9 from 9) -> (12) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(9 from 9) -> (3) 4\\r\\n\x00\x00\x00\x00\\r\\n (esc)
  write(3 from 5) -> (0) 0\r\n
  write limit reached; closing socket
  write(27) -> 15\r\nInternal Server Error\r\n

  $ rm -f error.log
  $ rm -rf clone
