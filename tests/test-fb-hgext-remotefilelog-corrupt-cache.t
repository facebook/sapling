  $ setconfig extensions.treemanifest=!

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
  $ chmod u+w $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

Verify corrupt cache error message

  $ hg up -q null

Enable delaywrite to avoid races when checking for corruption.
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > validatecache=off
  > [debug]
  > dirstate.delaywrite=1
  > EOF
  $ chmod u+w $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip 2>&1 | egrep "^RuntimeError"
  RuntimeError: unexpected remotefilelog header: illegal format

Verify detection and remediation when remotefilelog.validatecachelog is set

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > validatecachelog=$PWD/.hg/remotefilelog_cache.log
  > validatecache=strict
  > EOF
  $ chmod u+w $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ echo x > $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cat .hg/remotefilelog_cache.log
  corrupt $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0 during contains

Verify that hashes are checked
  $ rm .hg/remotefilelog_cache.log
  $ hg up -q null
  $ chmod u+w $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $ printf 'z' | dd of=$CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0 bs=1 seek=9 count=1 conv=notrunc 2> /dev/null
  $ hg up tip --config remotefilelog.validatecachehashes=False
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  z
  $ hg st --config remotefilelog.validatecachehashes=False

Verify that hashes are checked
  $ hg up -C -q null
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cat .hg/remotefilelog_cache.log
  corrupt $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0 during contains
  $ cat x
  x

Verify handling of corrupt server cache

  $ rm -f ../master/.hg/remotefilelogcache/y/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $ touch ../master/.hg/remotefilelogcache/y/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $ clearcache
  $ hg prefetch -r .
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ test -s ../master/.hg/remotefilelogcache/y/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $ hg debugremotefilelog $CACHEDIR/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  size: 2 bytes
  path: $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca 
  key: 076f5e2225b3 
  filename: y 
  
          node =>           p1            p2      linknode     copyfrom
  076f5e2225b3 => 000000000000  000000000000  f3d0bb0d1e48  

Verify some bad history data is caught and remediated even when validation is off
  $ chmod u+w $CACHEDIR/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $ $PYTHON $TESTDIR/truncate.py --size 20 $CACHEDIR/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $ hg log -f y --config remotefilelog.validatecache=off
  detected corruption in '$TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca', moving it aside
  changeset:   0:f3d0bb0d1e48
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     xy
  
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
