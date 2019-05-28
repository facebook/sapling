  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.flatcompat=False

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ cd master
  $ echo xx > x
  $ hg commit -qAm x2
  $ cd ..

# Test cache misses with read only permissions on server

  $ chmod -R a-w master/.hg/remotefilelogcache
  $ cd shallow
  $ hg pull -q
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cd ..

  $ chmod -R u+w master/.hg/remotefilelogcache

# Test setting up shared cache with the right permissions
# (this is hard to test in a cross platform way, so we just make sure nothing
# crashes)

  $ rm -rf $CACHEDIR
  $ umask 002
  $ mkdir $CACHEDIR
  $ hg -q clone --shallow ssh://user@dummy/master shallow2 --config remotefilelog.cachegroup="`id -g -n`"
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls -ld $CACHEDIR/reponame-default/11
  drwxrws* $TESTTMP/hgcache/reponame-default/11 (glob)
