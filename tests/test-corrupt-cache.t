  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ hg commit -qAm xy

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ cd shallow

Verify corrupt cache handling repairs by default

  $ hg up -q null
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

Verify corrupt cache error message

  $ hg up -q null
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > validatecache=off
  > EOF
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip 2>&1 | grep "corrupt cache data for"
      raise Exception("corrupt cache data for '%s'" % (self.filename))
  Exception: corrupt cache data for 'x'

Verify detection and remediation when remotefilelog.validatecachelog is set

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > validatecachelog=$PWD/.hg/remotefilelog_cache.log
  > validatecache=strict
  > EOF
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cat .hg/remotefilelog_cache.log
  corrupt $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0 during contains
