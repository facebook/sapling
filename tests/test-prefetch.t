  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo x2 > x
  $ echo y > y
  $ hg commit -qAm y

  $ cd ..

# prefetch a revision

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  2 files to transfer, 497 bytes of data
  transferred 497 bytes in 0.0 seconds (*/sec) (glob)
  $ cd shallow

  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x

# prefetch a range of revisions

  $ clearcache
  $ hg prefetch -r 0::1
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x
  $ hg cat -r 1 x
  x2

# prefetch certain files

  $ clearcache
  $ hg prefetch -r 1 x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 1 x
  x2

  $ hg cat -r 1 y
  y
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# prefetch on pull when configured

  $ printf "[remotefilelog]\npullprefetch=tip\n" >> .hg/hgrc
  $ hg strip tip^
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/b292c1e3311f-backup.hg

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over 0.09s

  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
