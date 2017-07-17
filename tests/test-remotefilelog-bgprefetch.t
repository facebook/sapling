  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
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
  $ hg bookmark foo

  $ cd ..

# prefetch a revision

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  2 files to transfer, 528 bytes of data
  transferred 528 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow

  $ hg prefetch -r 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x

# background prefetch on pull when configured

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > pullprefetch=bookmark()
  > backgroundprefetch=True
  > EOF
  $ hg strip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/109c3a557a73-3f43405e-backup.hg (glob)

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
  (running background prefetch)
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/ef95c5376f34698742fe34f315fd82136f8f68c0
  $TESTTMP/hgcache/master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a
  $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $TESTTMP/hgcache/repos

# background prefetch with repack on pull when configured

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > backgroundrepack=True
  > EOF
  $ hg strip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/109c3a557a73-3f43405e-backup.hg (glob)

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
  (running background prefetch)
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/47d8f1b90a73af4ff8af19fcd10bdc027b6a881a.histidx
  $TESTTMP/hgcache/master/packs/47d8f1b90a73af4ff8af19fcd10bdc027b6a881a.histpack
  $TESTTMP/hgcache/master/packs/830b4b13c2eeab214fdfe4635f27e3ad5e2dc730.dataidx
  $TESTTMP/hgcache/master/packs/830b4b13c2eeab214fdfe4635f27e3ad5e2dc730.datapack
  $TESTTMP/hgcache/repos
