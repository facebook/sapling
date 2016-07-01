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

2) Basic cache testing

  $ mkdir cachetesting
  $ cd cachetesting
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > cachecutoffdays=-1
  > debugmetrics=True
  > randomorder=False
  > EOF

  $ mkcommit a
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ mkcommit b
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ mkcommit c
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ mkcommit d
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ mkcommit e
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg diff -c . --debug --nodate
  [FM] cache miss for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache miss for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] other side is hybrid manifest
  [FM] cache miss
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', 0.0)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ hg debugcachemanifest -a
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 5)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
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
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg diff -c . --debug --nodate
  [FM] cache hit for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache hit for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] other side is hybrid manifest
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', 100.0)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Test the --pruneall command to prune all the cached manifests
  $ hg debugcachemanifest --pruneall --debug
  [FM] caching revset: [], pruneall(True), list(False)
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ hg diff -c . --debug --nodate
  [FM] cache miss for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache miss for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] other side is hybrid manifest
  [FM] cache miss
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', 0.0)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ cat >> .hg/hgrc << EOF
  > [fastmanifest]
  > cacheonchange=True
  > cacheonchangebackground=False
  > EOF
  $ mkcommit f
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: trigger, kwargs: [('source', 'commit')]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg book --debug foo
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: trigger, kwargs: [('source', 'bookmark')]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM] skipped 1853a742c28c3a531336bbb3d677d2e2d8937027, already cached (fast path)
  [FM] refreshing 1853a742c28c3a531336bbb3d677d2e2d8937027 with delay 0
  [FM] skipped 7ab5760d084a24168f7595c38c00f4bbc2e308d9, already cached (fast path)
  [FM] refreshing 7ab5760d084a24168f7595c38c00f4bbc2e308d9 with delay 0
  [FM] skipped f064a7f8e3e138341587096641d86e9d23cd9778, already cached (fast path)
  [FM] refreshing f064a7f8e3e138341587096641d86e9d23cd9778 with delay 0
  [FM] skipped e3738bf5439958f89499a656982023aba57b076e, already cached (fast path)
  [FM] refreshing e3738bf5439958f89499a656982023aba57b076e with delay 0
  [FM] skipped a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7, already cached (fast path)
  [FM] refreshing a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 with delay 0
  [FM] skipped a0c8bcbbb45c63b90b70ad007bf38961f64f2af0, already cached (fast path)
  [FM] refreshing a0c8bcbbb45c63b90b70ad007bf38961f64f2af0 with delay 0
  [FM] refreshing 1853a742c28c3a531336bbb3d677d2e2d8937027 with delay 0
  [FM] refreshing 7ab5760d084a24168f7595c38c00f4bbc2e308d9 with delay 2
  [FM] refreshing f064a7f8e3e138341587096641d86e9d23cd9778 with delay 4
  [FM] refreshing e3738bf5439958f89499a656982023aba57b076e with delay 6
  [FM] refreshing a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 with delay 8
  [FM] refreshing a0c8bcbbb45c63b90b70ad007bf38961f64f2af0 with delay 10
  [FM] nothing to do, cache size < limit
  $ hg diff -c . --debug --nodate
  [FM] cache hit for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] cache hit for fastmanifest 1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM] performing diff
  [FM] other side is hybrid manifest
  diff -r 9d206ffc875e1bc304590549be293be36821e66c -r bbc3e467917630e7d77cd77298e1027030972893 f
  --- /dev/null
  +++ b/f
  @@ -0,0 +1,1 @@
  +f
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', 100.0)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ hg debugcachemanifest --all --debug
  [FM] caching revset: ['fastmanifesttocache()'], pruneall(False), list(False)
  [FM] skipped 1853a742c28c3a531336bbb3d677d2e2d8937027, already cached (fast path)
  [FM] refreshing 1853a742c28c3a531336bbb3d677d2e2d8937027 with delay 0
  [FM] skipped 7ab5760d084a24168f7595c38c00f4bbc2e308d9, already cached (fast path)
  [FM] refreshing 7ab5760d084a24168f7595c38c00f4bbc2e308d9 with delay 0
  [FM] skipped f064a7f8e3e138341587096641d86e9d23cd9778, already cached (fast path)
  [FM] refreshing f064a7f8e3e138341587096641d86e9d23cd9778 with delay 0
  [FM] skipped e3738bf5439958f89499a656982023aba57b076e, already cached (fast path)
  [FM] refreshing e3738bf5439958f89499a656982023aba57b076e with delay 0
  [FM] skipped a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7, already cached (fast path)
  [FM] refreshing a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 with delay 0
  [FM] skipped a0c8bcbbb45c63b90b70ad007bf38961f64f2af0, already cached (fast path)
  [FM] refreshing a0c8bcbbb45c63b90b70ad007bf38961f64f2af0 with delay 0
  [FM] refreshing 1853a742c28c3a531336bbb3d677d2e2d8937027 with delay 0
  [FM] refreshing 7ab5760d084a24168f7595c38c00f4bbc2e308d9 with delay 2
  [FM] refreshing f064a7f8e3e138341587096641d86e9d23cd9778 with delay 4
  [FM] refreshing e3738bf5439958f89499a656982023aba57b076e with delay 6
  [FM] refreshing a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 with delay 8
  [FM] refreshing a0c8bcbbb45c63b90b70ad007bf38961f64f2af0 with delay 10
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --pruneall
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifesttocache()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --all --debug
  [FM] caching revset: ['fastmanifesttocache()'], pruneall(False), list(False)
  [FM] caching revision 1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM] cache miss for fastmanifest 1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM] caching revision 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] cache miss for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] caching revision f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache miss for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] caching revision e3738bf5439958f89499a656982023aba57b076e
  [FM] cache miss for fastmanifest e3738bf5439958f89499a656982023aba57b076e
  [FM] caching revision a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] cache miss for fastmanifest a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] caching revision a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] cache miss for fastmanifest a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] refreshing 1853a742c28c3a531336bbb3d677d2e2d8937027 with delay 0
  [FM] refreshing 7ab5760d084a24168f7595c38c00f4bbc2e308d9 with delay 2
  [FM] refreshing f064a7f8e3e138341587096641d86e9d23cd9778 with delay 4
  [FM] refreshing e3738bf5439958f89499a656982023aba57b076e with delay 6
  [FM] refreshing a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 with delay 8
  [FM] refreshing a0c8bcbbb45c63b90b70ad007bf38961f64f2af0 with delay 10
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifesttocache()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  cache size is: 1.50 KB
  number of entries is: 6
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  1853a742c28c3a531336bbb3d677d2e2d8937027|5
  7ab5760d084a24168f7595c38c00f4bbc2e308d9|4
  f064a7f8e3e138341587096641d86e9d23cd9778|3
  e3738bf5439958f89499a656982023aba57b076e|2
  a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7|1
  a0c8bcbbb45c63b90b70ad007bf38961f64f2af0|0
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Check that trimming to a limit higher than what is cached does nothing
  $ hg debugcachemanifest --debug --limit=2048
  [FM] caching revset: [], pruneall(False), list(False)
  [FM] nothing to do, cache size < limit
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: ondiskcachestats, kwargs: [('bytes', 1536), ('numentries', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Trim the cache to at most 1kb, we should start from the oldest entry to the
newest ones:
  $ hg debugcachemanifest --debug --limit=1024
  [FM] caching revset: [], pruneall(False), list(False)
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: ondiskcachestats, kwargs: [('bytes', 984), ('numentries', 3)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 984 bytes
  number of entries is: 3
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  1853a742c28c3a531336bbb3d677d2e2d8937027|5
  7ab5760d084a24168f7595c38c00f4bbc2e308d9|4
  f064a7f8e3e138341587096641d86e9d23cd9778|3
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  3
  4
  5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Trim the cache to at most 512 bytes
  $ hg debugcachemanifest --debug --limit=512
  [FM] caching revset: [], pruneall(False), list(False)
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: ondiskcachestats, kwargs: [('bytes', 376), ('numentries', 1)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  cache size is: 376 bytes
  number of entries is: 1
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  1853a742c28c3a531336bbb3d677d2e2d8937027|5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  5
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Trim the cache to at most 100 bytes
  $ hg debugcachemanifest --debug --limit=100
  [FM] caching revset: [], pruneall(False), list(False)
  [FM] removing cached manifest fast1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: ondiskcachestats, kwargs: [('bytes', 0), ('numentries', 0)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  cache size is: 0 bytes
  number of entries is: 0
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Check that trimming the cache to 0 byte works
  $ hg debugcachemanifest -a
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  cache size is: 1.50 KB
  number of entries is: 6
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  1853a742c28c3a531336bbb3d677d2e2d8937027|5
  7ab5760d084a24168f7595c38c00f4bbc2e308d9|4
  f064a7f8e3e138341587096641d86e9d23cd9778|3
  e3738bf5439958f89499a656982023aba57b076e|2
  a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7|1
  a0c8bcbbb45c63b90b70ad007bf38961f64f2af0|0
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --limit=0
  [FM] caching revset: [], pruneall(False), list(False)
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] removing cached manifest fast1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: ondiskcachestats, kwargs: [('bytes', 0), ('numentries', 0)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  cache size is: 0 bytes
  number of entries is: 0
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

Use the cache in a commit.
  $ hg debugcachemanifest -a
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: revsetsize, kwargs: [('size', 6)]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  $ mkcommit g
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', -1)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics
  [FM-METRICS] Begin metrics
  [FM-METRICS] kind: trigger, kwargs: [('source', 'commit')]
  [FM-METRICS] kind: trigger, kwargs: [('source', 'bookmark')]
  [FM-METRICS] kind: cachehitratio, kwargs: [('cachehitratio', 100.0)]
  [FM-METRICS] kind: diffcachehitratio, kwargs: [('diffcachehitratio', -1)]
  [FM-METRICS] kind: filesnotincachehitratio, kwargs: [('filesnotincachehitratio', -1)]
  [FM-METRICS] End metrics

  $ cd ..
