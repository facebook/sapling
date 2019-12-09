#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo y >> x
  $ hg commit -qAm y
  $ mkdir dir
  $ echo z >> dir/z
  $ hg commit -qAm z
  $ echo z >> dir/z2
  $ hg commit -qAm z2

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over 0.00s
  $ cd shallow

Test blame

  $ clearcache
  $ hg archive -r tip -t tar myarchive.tar
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over 0.00s
