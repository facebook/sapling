#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ echo x > x
  $ echo z > z
  $ hg commit -qAm x1
  $ echo x2 > x
  $ echo z2 > z
  $ hg commit -qAm x2
  $ hg bookmark foo

  $ cd ..

# prefetch a revision w/ a sparse checkout

  $ clone master shallow --noupdate
  $ cd shallow
  $ printf "[extensions]\nsparse=\n" >> .hg/hgrc

  $ hg sparse -I x
  $ hg prefetch -r 'desc(x1)'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg cat -r 'desc(x1)' x
  x

  $ hg sparse -I z
  $ hg prefetch -r 'desc(x1)'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg cat -r 'desc(x1)' z
  z

# prefetch sparse only on pull when configured

  $ printf "[remotefilelog]\npullprefetch=bookmark()\n" >> .hg/hgrc
  $ hg debugstrip tip

  $ hg sparse --delete z

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  prefetching file contents
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# Dont consider filtered files when doing copy tracing

## Push an unrelated commit
  $ cd ../

  $ clone master shallow2
  $ cd shallow2
  $ printf "[extensions]\nsparse=\n" >> .hg/hgrc

  $ hg up -q 'desc(x1)'
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)
  $ touch a
  $ hg ci -Aqm a
  $ hg push -q -f --allow-anon

## Pull the unrelated commit and rebase onto it - verify unrelated file was not
pulled

  $ cd ../shallow
  $ hg up -q 'desc(x2)'
  $ hg pull -q
  $ hg sparse -I z
  $ clearcache
  $ hg prefetch -r '. + .^' -I x -I z
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over * (glob) (?)
  $ hg rebase -d 'desc(a)' --keep
  rebasing 876b1317060d "x2" (default/foo foo)
