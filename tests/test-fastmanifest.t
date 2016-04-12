Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }


  $ mkdir diagnosis
  $ cd diagnosis
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > logfile=$TESTTMP/logfile
  > EOF


2) Basic

  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg diff -c . --debug --nodate
  performing diff
  other side is hybrid manifest
  fallback to regular diff
  cache miss for flatmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  cache miss for flatmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e

  $ hg debugcachemanifest -a --debug --flat
  caching rev: <addset <baseset+ [0, 1, 2, 3, 4]>, <baseset+ []>> , synchronous(False), flat(True)
  caching revision a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  cache miss for flatmanifest a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  caching revision a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  cache miss for flatmanifest a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  caching revision e3738bf5439958f89499a656982023aba57b076e
  cache miss for flatmanifest e3738bf5439958f89499a656982023aba57b076e
  caching revision f064a7f8e3e138341587096641d86e9d23cd9778
  cache miss for flatmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  caching revision 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  cache miss for flatmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9



  $ hg diff -c . --debug --nodate
  performing diff
  other side is hybrid manifest
  fallback to regular diff
  cache hit for flatmanifest f064a7f8e3e138341587096641d86e9d23cd9778
  cache hit for flatmanifest 7ab5760d084a24168f7595c38c00f4bbc2e308d9
  diff -r 47d2a3944de8b013de3be9578e8e344ea2e6c097 -r 9d206ffc875e1bc304590549be293be36821e66c e
  --- /dev/null
  +++ b/e
  @@ -0,0 +1,1 @@
  +e

