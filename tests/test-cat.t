  $ mkdir t
  $ cd t
  $ hg init
  $ echo 0 > a
  $ echo 0 > b
  $ hg ci -A -m m -d "1000000 0"
  adding a
  adding b
  $ hg rm a
  $ hg cat a
  0
  $ hg cat --decode a # more tests in test-encode
  0
  $ echo 1 > b
  $ hg ci -m m -d "1000000 0"
  $ echo 2 > b
  $ hg cat -r 0 a
  0
  $ hg cat -r 0 b
  0
  $ hg cat -r 1 a
  a: No such file in rev 03f6b0774996
  $ hg cat -r 1 b
  1
