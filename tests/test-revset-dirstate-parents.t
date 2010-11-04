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
  ('func', ('symbol', 'p1'), None)
  -1
  $ try 'p2()'
  ('func', ('symbol', 'p2'), None)

null revision
  $ log 'p1()'
  $ log 'p2()'
  $ log 'parents()'

working dir with a single parent
  $ echo a > a
  $ hg ci -Aqm0
  $ log 'p1()'
  0
  $ log 'p2()'
  $ log 'parents()'
  0

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
  $ log 'parents()'
  2
  1
