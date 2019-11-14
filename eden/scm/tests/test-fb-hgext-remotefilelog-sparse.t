  $ setconfig extensions.treemanifest=!

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
  3 files to transfer, * of data (glob)
  transferred 527 bytes in * seconds (*/sec) (glob)
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
  $ hg debugstrip tip
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
  new changesets 876b1317060d
  prefetching file contents
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# Dont consider filtered files when doing copy tracing

## Push an unrelated commit
  $ cd ../

  $ hgcloneshallow ssh://user@dummy/master shallow2
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred 527 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cd shallow2
  $ printf "[extensions]\nsparse=\n" >> .hg/hgrc

  $ hg up -q 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ touch a
  $ hg ci -Aqm a
  $ hg push -q -f

## Pull the unrelated commit and rebase onto it - verify unrelated file was not
pulled

  $ cd ../shallow
  $ hg up -q 1
  $ hg pull -q
  $ hg sparse -I z
  $ clearcache
  $ hg prefetch -r '. + .^' -I x -I z
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over * (glob)
  $ hg rebase -d 2 --keep
  rebasing 876b1317060d "x2" (foo)
