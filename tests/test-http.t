  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
Bug: remotefilelog tries to fetch from a server which can't handle
remotefilelog requests because it's http-based:
  $ hg serve -p $HGPORT -d --pid-file=../hg1.pid -E ../error.log

  $ cd ..
  $ cat hg1.pid >> $DAEMON_PIDS

  $ hgcloneshallow http://localhost:$HGPORT/ shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  abort: HTTP Error 500: Internal Server Error

Bug: remotefilelog capability is advertised over http even though it
won't work.
  $ get-with-headers.py localhost:$HGPORT '?cmd=capabilities'
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch stream-preferred stream bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 remotefilelog (no-eol)
  $ get-with-headers.py localhost:$HGPORT '?cmd=hello'
  200 Script output follows
  
  capabilities: lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch stream-preferred stream bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 remotefilelog

  $ get-with-headers.py localhost:$HGPORT '?cmd=this-command-does-not-exist' | head -n 1
  400 no such method: this-command-does-not-exist
Bug: this should return 400, not 5xx
  $ get-with-headers.py localhost:$HGPORT '?cmd=getfiles' | head -n 1
  500 Internal Server Error
