#chg-compatible
#debugruntest-compatible

  $ setconfig format.dirstate=2

  $ HGENCODING=utf-8
  $ export HGENCODING

  $ try() {
  >   hg debugrevspec --debug $@
  > }

  $ log() {
  >   hg log --template '{node}\n' -r "$1"
  > }

  $ hg init repo
  $ cd repo

  $ try 'p1()'
  (func
    (symbol 'p1')
    None)
  * set:
  <baseset []>
  $ try 'p2()'
  (func
    (symbol 'p2')
    None)
  * set:
  <baseset []>
  $ try 'parents()'
  (func
    (symbol 'parents')
    None)
  * set:
  <baseset- []>

null revision
  $ log 'p1()'
  $ log 'p2()'
  $ log 'parents()'

working dir with a single parent
  $ echo a > a
  $ hg ci -Aqm0
  $ log 'p1()'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac
  $ log 'p2()'
  $ log 'parents()'
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

merge in progress
  $ echo b > b
  $ hg ci -Aqm1
  $ hg up -q 'desc(0)'
  $ echo c > c
  $ hg ci -Aqm2
  $ hg merge -q
  $ log 'p1()'
  db815d6d32e69058eadefc8cffbad37675707975
  $ log 'p2()'
  925d80f479bb026b0fb3deb27503780b13f74123
  $ log 'parents()'
  925d80f479bb026b0fb3deb27503780b13f74123
  db815d6d32e69058eadefc8cffbad37675707975

  $ cd ..
