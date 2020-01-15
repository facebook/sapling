#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ LFSPATH=$TESTTMP/lfs
  $ export LFSPATH
  $ mkdir $LFSPATH

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > lfs=
  > [lfs]
  > url=file://$LFSPATH
  > verify=existance
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo z > z
  $ hg commit -qAm x
  $ echo x2 > x
  $ echo y > y
  $ hg commit -qAm y
  $ echo large > large
  $ hg --config 'lfs.threshold=1' commit -qAm y
  $ hg bookmark foo
  $ hg debuglfsupload -r tip

  $ cd ..

# prefetch a revision

  $ hgcloneshallowlfs ssh://user@dummy/master shallow file://$LFSPATH --noupdate
  streaming all changes
  3 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  $ cd shallow

  $ hg prefetch -r 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg cat -r 0 x
  x

# prefetch a range of revisions

  $ clearcache
  $ hg prefetch -r 0::1
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg cat -r 0 x
  x
  $ hg cat -r 1 x
  x2

# prefetch certain files

  $ clearcache
  $ hg prefetch -r 1 x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg cat -r 1 x
  x2

  $ hg cat -r 1 y
  y
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# prefetch large file

  $ hg prefetch -r 2
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)

# prefetch on pull when configured

  $ printf "[remotefilelog]\npullprefetch=bookmark()\n" >> .hg/hgrc
  $ hg debugstrip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/730e2b7b175c-acada81e-backup.hg (glob)

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  prefetching file contents
  4 files fetched over * fetches - (4 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg up tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
