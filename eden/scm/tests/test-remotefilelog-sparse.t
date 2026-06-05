#chg-compatible
#require no-eden

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ echo x > x
  $ echo z > z
  $ sl commit -qAm x1
  $ echo x2 > x
  $ echo z2 > z
  $ sl commit -qAm x2
  $ sl bookmark master

  $ cd ..

# prefetch a revision w/ a sparse checkout

  $ clone master shallow --noupdate
  $ cd shallow
  $ printf "[extensions]\nsparse=\n" >> .sl/config

  $ sl sparse -I x
  $ sl prefetch -r 'desc(x1)'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ sl cat -r 'desc(x1)' x
  x

  $ sl sparse -I z
  $ sl prefetch -r 'desc(x1)'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ sl cat -r 'desc(x1)' z
  z

# prefetch sparse only on pull when configured

  $ printf "[remotefilelog]\npullprefetch=bookmark()\n" >> .sl/config
  $ sl debugstrip tip

  $ sl sparse --delete z

  $ clearcache
  $ sl pull
  pulling from (ssh://user@dummy/|test:)master (re)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  imported commit graph for 1 commit (1 segment)
  prefetching file contents

# Dont consider filtered files when doing copy tracing

## Push an unrelated commit
  $ cd ../

  $ clone master shallow2
  $ cd shallow2
  $ printf "[extensions]\nsparse=\n" >> .sl/config

  $ sl up -q 'desc(x1)'
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)
  $ touch a
  $ sl ci -Aqm a
  $ sl push -q -f --allow-anon
  $ sl whereami
  a96546d1acd84e2abf205545385ed16a0b4c3337

## Pull the unrelated commit and rebase onto it - verify unrelated file was not
pulled

  $ cd ../shallow
  $ sl up -q 'desc(x2)'
  $ sl pull -q -r a96546d1acd
  $ sl sparse -I z
  $ clearcache
  $ sl prefetch -r '. + .^' -I x -I z
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over * (glob) (?)
  $ sl rebase -d 'desc(a)' --keep
  rebasing 876b1317060d "x2" (remote/master master)

# prefetch with explicit patterns should still respect sparse profile

  $ cd ../
  $ clone master shallow3 --noupdate
  $ cd shallow3
  $ printf "[extensions]\nsparse=\n" >> .sl/config

  $ sl sparse -I x
  $ clearcache

# Prefetch with explicit pattern that includes excluded file should not fetch excluded file
  $ sl prefetch -r 'desc(x1)' -I x -I z
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# Verify that only x was fetched (since z is excluded by sparse profile)
  $ sl cat -r 'desc(x1)' x
  x

# Now include z in sparse profile and prefetch again
  $ sl sparse -I z
  $ clearcache
  $ sl prefetch -r 'desc(x1)' -I z
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# Verify z is now fetched
  $ sl cat -r 'desc(x1)' z
  z
