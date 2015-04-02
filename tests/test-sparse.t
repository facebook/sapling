  $ python -c 'import sparse' || exit 80

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo z > z
  $ hg commit -qAm x1
  $ echo x2 > x
  $ echo z2 > z
  $ hg commit -qAm x2
  $ hg bookmark foo

  $ cd ..

# prefetch a revision w/ a sparse checkout

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  2 files to transfer, 527 bytes of data
  transferred 527 bytes in 0.0 seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow
  $ printf "[extensions]\nsparse=\n" >> .hg/hgrc

  $ hg sparse -I x
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x

  $ hg sparse -I z
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 z
  z

# prefetch sparse only on pull when configured

  $ printf "[remotefilelog]\npullprefetch=bookmark()\n" >> .hg/hgrc
  $ hg strip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/876b1317060d-b2e91d8d-backup.hg (glob)

  $ hg sparse --delete z

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  (run 'hg update' to get a working copy)
  prefetching file contents
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
