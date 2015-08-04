  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ hg serve -p $HGPORT -d --pid-file=../hg1.pid -E ../error.log

Build a query string for later use:
  $ GET=`hg debugdata -m 0 | python -c \
  > 'import sys ; print [("?cmd=getfile&file=%s&node=%s" % tuple(s.split("\0"))) for s in sys.stdin.read().splitlines()][0]'`

  $ cd ..
  $ cat hg1.pid >> $DAEMON_PIDS

  $ hgcloneshallow http://localhost:$HGPORT/ shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)


The 'remotefilelog' capability should *not* be exported over http(s),
as the getfile method it offers doesn't work with http.
  $ get-with-headers.py localhost:$HGPORT '?cmd=capabilities'
  200 Script output follows
  
  lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch stream-preferred stream bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 getfile (no-eol)
  $ get-with-headers.py localhost:$HGPORT '?cmd=hello'
  200 Script output follows
  
  capabilities: lookup changegroupsubset branchmap pushkey known getbundle unbundlehash batch stream-preferred stream bundle2=HG20%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0Aerror%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0Apushkey%0Aremote-changegroup%3Dhttp%2Chttps unbundle=HG10GZ,HG10BZ,HG10UN httpheader=1024 getfile

  $ get-with-headers.py localhost:$HGPORT '?cmd=this-command-does-not-exist' | head -n 1
  400 no such method: this-command-does-not-exist
  $ get-with-headers.py localhost:$HGPORT '?cmd=getfiles' | head -n 1
  400 no such method: getfiles

Verify serving from a shallow clone doesn't allow for remotefile
fetches. This also serves to test the error handling for our batchable
getfile RPC.

  $ cd shallow
  $ hg serve -p $HGPORT1 -d --pid-file=../hg2.pid -E ../error2.log
  $ cd ..
  $ cat hg2.pid >> $DAEMON_PIDS

This GET should work, because this server is serving master, which is
a full clone.

  $ get-with-headers.py localhost:$HGPORT "$GET"
  200 Script output follows
  
  0\x00U\x00\x00\x00\xff (esc)
  2\x00x (esc)
  \x14\x06\xe7A\x18bv\x94&\x84\x17I\x1f\x01\x8aJ\x881R\xf0\x00\x01\x00\x14\xf0\x06\xb2\x92\xc1\xe31\x1f\xd0\xf1:\xe8;@\x9c\xaa\xe4\xa6\xd1\xfb4\x8c\x00 (no-eol) (esc)

This GET should fail using the in-band signalling mechanism, because
it's not a full clone. Note that it's also plausible for servers to
refuse to serve file contents for other reasons, like the file
contents not being visible to the current user.

  $ get-with-headers.py localhost:$HGPORT1 "$GET"
  200 Script output follows
  
  1\x00cannot fetch remote files from shallow repo (no-eol) (esc)

Both error logs should be empty:
  $ cat error.log
  $ cat error2.log
