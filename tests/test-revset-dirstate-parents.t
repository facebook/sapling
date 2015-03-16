  $ HGENCODING=utf-8
  $ export HGENCODING

  $ try() {
  >   hg debugrevspec --debug $@
  > }

  $ log() {
  >   hg log --template '{rev}\n' -r "$1"
  > }

  $ hg init repo
  $ cd repo

  $ try 'p1()'
  (func
    ('symbol', 'p1')
    None)
  * set:
  <baseset []>
  $ try 'p2()'
  (func
    ('symbol', 'p2')
    None)
  * set:
  <baseset []>
  $ try 'parents()'
  (func
    ('symbol', 'parents')
    None)
  * set:
  <baseset+ []>

null revision
  $ log 'p1()'
  $ log 'p2()'
  $ log 'parents()'

working dir with a single parent
  $ echo a > a
  $ hg ci -Aqm0
  $ log 'p1()'
  0
  $ log 'tag() and p1()'
  $ log 'p2()'
  $ log 'parents()'
  0
  $ log 'tag() and parents()'

merge in progress
  $ echo b > b
  $ hg ci -Aqm1
  $ hg up -q 0
  $ echo c > c
  $ hg ci -Aqm2
  $ hg merge -q
  $ log 'p1()'
  2
  $ log 'p2()'
  1
  $ log 'tag() and p2()'
  $ log 'parents()'
  1
  2

  $ cd ..
