  $ hg init
  $ echo 0 > a
  $ echo 0 > b
  $ hg ci -A -m m
  adding a
  adding b
  $ hg rm a
  $ hg cat a
  0
  $ hg cat --decode a # more tests in test-encode
  0
  $ echo 1 > b
  $ hg ci -m m
  $ echo 2 > b
  $ hg cat -r 0 a
  0
  $ hg cat -r 0 b
  0
  $ hg cat -r 1 a
  a: no such file in rev 7040230c159c
  [1]
  $ hg cat -r 1 b
  1
