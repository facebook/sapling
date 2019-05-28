  $ setconfig extensions.treemanifest=!
Setup


Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }

2) Set up the repo

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
  $ hg debugcachemanifest -a
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

3) Test commits based off of revisions that are in the cache

  $ mkcommit f
  $ hg diff -c . --config extensions.fastmanifest=!
  diff -r 9d206ffc875e -r bbc3e4679176 f
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +f

  $ hg debugcachemanifest -a
  $ hg debugcachemanifest --list
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
  $ echo "ff" >> f
  $ hg commit -m "amended" --debug | grep 'wrote manifest'
  [FM] wrote manifest 2545498ff92a8988bcc2173b6b21f181449e4d68

  $ hg log -r .
  changeset:   6:263e90ebaf7a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     amended
  
  $ hg diff -c . --config extensions.fastmanifest=!
  diff -r bbc3e4679176 -r 263e90ebaf7a f
  --- a/f	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +ff
