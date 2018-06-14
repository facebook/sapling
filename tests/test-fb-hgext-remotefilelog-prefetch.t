
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
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow

  $ hg prefetch -r 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x

# prefetch with base

  $ clearcache
  $ hg prefetch -r 0::1 -b 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 1 x
  x2
  $ hg cat -r 1 y
  y

  $ hg cat -r 0 x
  x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 z
  z
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ hg prefetch -r 0::1 --base 0
  $ hg prefetch -r 0::1 -b 1
  $ hg prefetch -r 0::1

# prefetch a range of revisions

  $ clearcache
  $ hg prefetch -r 0::1
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over *s (glob)

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

  $ printf "[remotefilelog]\npullprefetch=bookmark()\n" >> .hg/hgrc
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
  new changesets 109c3a557a73
  (run 'hg update' to get a working copy)
  prefetching file contents
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

# prefetch only fetches changes not in working copy

  $ hg strip tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/109c3a557a73-3f43405e-backup.hg (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ clearcache

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  new changesets 109c3a557a73
  (run 'hg update' to get a working copy)
  prefetching file contents
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

# Make some local commits that produce the same file versions as are on the
# server. To simulate a situation where we have local commits that were somehow
# pushed, and we will soon pull.

  $ hg prefetch -r 'all()'
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ hg strip -q -r 0
  $ echo x > x
  $ echo z > z
  $ hg commit -qAm x
  $ echo x2 > x
  $ echo y > y
  $ hg commit -qAm y

# prefetch server versions, even if local versions are available

  $ clearcache
  $ hg strip -q tip
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  new changesets 109c3a557a73
  (run 'hg update' to get a working copy)
  prefetching file contents
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ cd ..

# Prefetch unknown files during checkout

  $ hgcloneshallow ssh://user@dummy/master shallow2
  streaming all changes
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds * (glob)
  searching for changes
  no changes found
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd shallow2
  $ hg up -q null
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ clearcache
  $ hg up tip
  x: untracked file differs
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob)
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg revert --all

# Test batch fetching of lookup files during hg status
  $ hg up --clean tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugrebuilddirstate
  $ clearcache
  $ hg status
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob)

# Prefetch during addrename detection
  $ hg up -q --clean tip
  $ hg revert --all
  $ mv x x2
  $ mv y y2
  $ mv z z2
  $ clearcache
  $ hg addremove -s 50 > /dev/null
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob)

  $ cd ..

# Prefetch packs
  $ hgcloneshallow ssh://user@dummy/master packprefetch
  streaming all changes
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd packprefetch
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > backgroundrepack=True
  > EOF
  $ clearcache
  $ hg prefetch -r .
  3 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ find $TESTTMP/hgcache -type f | sort
  $TESTTMP/hgcache/master/packs/47d8f1b90a73af4ff8af19fcd10bdc027b6a881a.histidx
  $TESTTMP/hgcache/master/packs/47d8f1b90a73af4ff8af19fcd10bdc027b6a881a.histpack
  $TESTTMP/hgcache/master/packs/8c654541e4f20141a894bbfe428e36fc92202e39.dataidx
  $TESTTMP/hgcache/master/packs/8c654541e4f20141a894bbfe428e36fc92202e39.datapack
  $ hg cat -r . x
  x2
  $ hg cat -r . y
  y
  $ hg cat -r . z
  z

# Prefetch packs that include renames
  $ cd ../master
  $ hg mv z z2
  $ hg commit -m 'move z -> z2'
  $ cd ../packprefetch
  $ hg pull -q
  (running background incremental repack)
  $ hg prefetch -r tip
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg up tip -q
  $ hg log -f z2 -T '{desc}\n'
  move z -> z2
  x

# Revert across double renames. Note: the scary "abort", error is because
# https://bz.mercurial-scm.org/5419 .

  $ clearcache
  $ hg mv y y2
  $ hg mv x x2
  $ hg mv z2 z3
  $ hg revert -a -r 1 || true
  undeleting x
  forgetting x2
  undeleting y
  forgetting y2
  adding z
  forgetting z3
  3 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  abort: z2@109c3a557a73: not found in manifest! (?)

# Test connection pool lifetime
  $ clearcache
  $ hg prefetch -r 0::1 --debug --config connectionpool.lifetime=0 | grep 'closing expired connection'
  4 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  closing expired connection to ssh://user@dummy/master
  $ clearcache
  $ hg prefetch -r 0::1 --debug --config connectionpool.lifetime=300 | grep 'closing expired connection'
  4 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  [1]

  $ cat >$TESTTMP/testpool <<EOF
  > import time
  > with repo.connectionpool.get('ssh://user@dummy/master') as conn:
  >     connid = id(conn)
  >     print "got first connection"
  > with repo.connectionpool.get('ssh://user@dummy/master') as conn:
  >     assert connid == id(conn)
  >     print "got second connection"
  > time.sleep(2)
  > with repo.connectionpool.get('ssh://user@dummy/master') as conn:
  >     assert connid != id(conn)
  >     print "got third connection"
  >     time.sleep(2)
  > EOF
  $ hg debugshell --command "`cat $TESTTMP/testpool`" --config connectionpool.lifetime=1 --debug | grep 'connection'
  got first connection
  got second connection
  not reusing expired connection to ssh://user@dummy/master
  got third connection
  closing expired connection to ssh://user@dummy/master
