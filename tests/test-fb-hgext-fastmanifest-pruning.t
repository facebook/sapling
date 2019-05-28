  $ setconfig extensions.treemanifest=!
Setup


Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }

2a) Create a repo with a bunch of revs.

  $ mkdir pruning
  $ cd pruning
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > cachecutoffdays=-1
  > EOF
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg debugcachemanifest --list
  cache size is: 0 bytes
  number of entries is: 0

2b) Bring everything into the cache.

  $ hg debugcachemanifest --all
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

2c) Make more commits.

  $ mkcommit f

2d) Try to bring more entries into the cache, but with a limit, forcing
out *some* the older revisions.

  $ hg debugcachemanifest --all --limit 1024
  $ hg debugcachemanifest --list
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

