#chg-compatible

  $ setconfig extensions.treemanifest=!

no-check-code
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

Verify error message when no cachepath specified
  $ hg up -q null
  $ cp $HGRCPATH $HGRCPATH.bak
  $ sed -i.bak -n "/cachepath/!p" $HGRCPATH
  $ hg up tip
  abort: could not find config option remotefilelog.cachepath
  [255]
  $ mv $HGRCPATH.bak $HGRCPATH

Verify error message when no fallback specified

  $ hg up -q null
  $ rm .hg/hgrc
  $ clearcache
  $ hg up tip
  abort: no remotefilelog server configured - is your .hg/hgrc trusted?
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  [255]
