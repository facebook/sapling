Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }

2) Create a repo with a bunch of revs.

  $ mkdir concurrency
  $ cd concurrency
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > cachecutoffdays=-1
  > cacheonchange=True
  > cacheonchangebackground=True
  > EOF
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
this is a barrier to ensure that any cache workers that we've kicked off
have completed.
  $ python "$TESTDIR/waitforfile.py" --created .hg/store/manifestcache/fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 --deleted .hg/fastmanifest --max-time 1
  $ hg debugcachemanifest --pruneall
  $ hg debugcachemanifest --list
  cache size is: 0 bytes
  number of entries is: 0

3) Create a bookmark, triggering a cache fill.

  $ hg bookmark abc
this is a barrier to ensure that the cache fill has completed.
  $ python "$TESTDIR/waitforfile.py" --created .hg/store/manifestcache/fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 --deleted .hg/fastmanifest --max-time 1
  $ hg debugcachemanifest --list
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  cache size is: 1.13 KB
  number of entries is: 5
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  7ab5760d084a24168f7595c38c00f4bbc2e308d9|4
  f064a7f8e3e138341587096641d86e9d23cd9778|3
  e3738bf5439958f89499a656982023aba57b076e|2
  a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7|1
  a0c8bcbbb45c63b90b70ad007bf38961f64f2af0|0
remove the bookmark to restore the state, but don't cache on change,
because that could race with the --pruneall.
  $ hg boo -d abc --config=fastmanifest.cacheonchange=False
  $ hg debugcachemanifest --pruneall
  $ hg debugcachemanifest --list
  cache size is: 0 bytes
  number of entries is: 0

4) Fake a lock so it looks like another worker is caching entries, and
trigger a cache fill.

  $ touch .hg/fastmanifest
  $ hg bookmark abc
wait one second to ensure that the background worker started up and had a
chance to try to take the lock.
  $ sleep 1
  $ hg debugcachemanifest --list
  cache size is: 0 bytes
  number of entries is: 0
