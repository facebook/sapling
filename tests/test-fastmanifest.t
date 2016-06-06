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
  > randomorder=False
  > EOF

  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg diff -c . --debug --nodate
  [FM] cache miss for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache miss for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] diff: other side is hybrid manifest
  [FM] diff: cache miss
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e

Stress test to see if all these can work concurrently, if this test fails
there is a concurrency issue to address
  $ hg debugcachemanifest -a --background >/dev/null
  $ hg debugcachemanifest --pruneall --background >/dev/null
  $ hg debugcachemanifest --pruneall --background >/dev/null
  $ hg debugcachemanifest -a --background >/dev/null
  $ sleep 1
  $ hg debugcachemanifest -a --background
  $ hg debugcachemanifest -a --background
  $ hg debugcachemanifest -a --background
  $ sleep 1
  $ hg debugcachemanifest --list
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 1.13 KB
  number of entries is: 5
  $ hg diff -c . --debug --nodate
  [FM] cache hit for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache hit for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] diff: other side is hybrid manifest
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e

Test the --pruneall command to prune all the cached manifests
  $ hg debugcachemanifest --pruneall --debug
  [FM] caching revset: [], background(False), pruneall(True), list(False)
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778

  $ hg diff -c . --debug --nodate
  [FM] cache miss for fastmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  [FM] cache miss for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] performing diff
  [FM] diff: other side is hybrid manifest
  [FM] diff: cache miss
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e

  $ cat >> .hg/hgrc << EOF
  > [fastmanifest]
  > cacheonchange=True
  > cacheonchangebackground=False
  > EOF
  $ mkcommit f
  $ hg book --debug foo
  [FM] skipped 1853a742c28c3a531336bbb3d677d2e2d8937027, already cached (fast path)
  [FM] skipped 7ab5760d084a24168f7595c38c00f4bbc2e308d9, already cached (fast path)
  [FM] skipped f064a7f8e3e138341587096641d86e9d23cd9778, already cached (fast path)
  [FM] skipped e3738bf5439958f89499a656982023aba57b076e, already cached (fast path)
  [FM] skipped a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7, already cached (fast path)
  [FM] skipped a0c8bcbbb45c63b90b70ad007bf38961f64f2af0, already cached (fast path)
  [FM] nothing to do, cache size < limit
  $ hg diff -c . --debug --nodate
  [FM] cache hit for fastmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] cache hit for fastmanifest 1853a742c28c3a531336bbb3d677d2e2d8937027
  [FM] performing diff
  [FM] diff: other side is hybrid manifest
  diff -r 9d206ffc875e1bc304590549be293be36821e66c -r bbc3e467917630e7d77cd77298e1027030972893 f
  --- /dev/null
  +++ b/f
  @@ -0,0 +1,1 @@
  +f

  $ hg debugcachemanifest --all --debug
  [FM] caching revset: ['fastmanifesttocache()'], background(False), pruneall(False), list(False)
  [FM] skipped 1853a742c28c3a531336bbb3d677d2e2d8937027, already cached (fast path)
  [FM] skipped 7ab5760d084a24168f7595c38c00f4bbc2e308d9, already cached (fast path)
  [FM] skipped f064a7f8e3e138341587096641d86e9d23cd9778, already cached (fast path)
  [FM] skipped e3738bf5439958f89499a656982023aba57b076e, already cached (fast path)
  [FM] skipped a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7, already cached (fast path)
  [FM] skipped a0c8bcbbb45c63b90b70ad007bf38961f64f2af0, already cached (fast path)
  $ hg debugcachemanifest --pruneall
  $ hg log -r "fastmanifesttocache()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  $ hg debugcachemanifest --all --debug
  [FM] caching revset: ['fastmanifesttocache()'], background(False), pruneall(False), list(False)
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
  $ hg log -r "fastmanifesttocache()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  0
  1
  2
  3
  4
  5

Make the entries of the cache be in a deterministic order accross platforms
to make the test deterministic:

  >>> import os
  >>> files = sorted(os.listdir(".hg/store/manifestcache/"))
  >>> basetime = 1464039920
  >>> for fi in files:
  ...   f = os.path.join(".hg/store/manifestcache", fi)
  ...   os.utime(f, (basetime, basetime))
  ...   assert os.path.getmtime(f) == basetime
  ...   basetime+=10
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 1.50 KB
  number of entries is: 6

Check that trimming to a limit higher than what is cached does nothing
  $ hg debugcachemanifest --debug --limit=2048
  [FM] caching revset: [], background(False), pruneall(False), list(False)
  [FM] nothing to do, cache size < limit

Trim the cache to at most 1kb
  $ hg debugcachemanifest --debug --limit=1024
  [FM] caching revset: [], background(False), pruneall(False), list(False)
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] removing cached manifest fast1853a742c28c3a531336bbb3d677d2e2d8937027
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 832 bytes
  number of entries is: 4
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  0
  1
  2
  3

Trim the cache to at most 512 bytes
  $ hg debugcachemanifest --debug --limit=512
  [FM] caching revset: [], background(False), pruneall(False), list(False)
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 512 bytes
  number of entries is: 2
  $ hg log -r "fastmanifestcached()" -T '{rev}\n'
  2
  3

Trim the cache to at most 100 bytes
  $ hg debugcachemanifest --debug --limit=100
  [FM] caching revset: [], background(False), pruneall(False), list(False)
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  cache size is: 0 bytes
  number of entries is: 0

Check that trimming the cache to 0 byte works
  $ hg debugcachemanifest -a
Make the results deterministic
  >>> import os
  >>> files = sorted(os.listdir(".hg/store/manifestcache/"))
  >>> basetime = 1464039920
  >>> for fi in files:
  ...   f = os.path.join(".hg/store/manifestcache", fi)
  ...   os.utime(f, (basetime, basetime))
  ...   assert os.path.getmtime(f) == basetime
  ...   basetime+=10
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  fast1853a742c28c3a531336bbb3d677d2e2d8937027 (size 376 bytes)
  fast7ab5760d084a24168f7595c38c00f4bbc2e308d9 (size 328 bytes)
  fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0 (size 136 bytes)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  fastf064a7f8e3e138341587096641d86e9d23cd9778 (size 280 bytes)
  cache size is: 1.50 KB
  number of entries is: 6
  $ hg debugcachemanifest --debug --limit=0
  [FM] caching revset: [], background(False), pruneall(False), list(False)
  [FM] removing cached manifest fastf064a7f8e3e138341587096641d86e9d23cd9778
  [FM] removing cached manifest faste3738bf5439958f89499a656982023aba57b076e
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  [FM] removing cached manifest fasta0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  [FM] removing cached manifest fast7ab5760d084a24168f7595c38c00f4bbc2e308d9
  [FM] removing cached manifest fast1853a742c28c3a531336bbb3d677d2e2d8937027
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], background(False), pruneall(False), list(True)
  cache size is: 0 bytes
  number of entries is: 0

Use the cache in a commit.
  $ hg debugcachemanifest -a
  $ mkcommit g

  $ cd ..
