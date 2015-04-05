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

Test multiple files

  $ echo 3 > c
  $ hg ci -Am addmore c
  $ hg cat b c
  1
  3
  $ hg cat .
  1
  3
  $ hg cat . c
  1
  3

Test fileset

  $ hg cat 'set:not(b) or a'
  3
  $ hg cat 'set:c or b'
  1
  3

  $ mkdir tmp
  $ hg cat --output tmp/HH_%H c
  $ hg cat --output tmp/RR_%R c
  $ hg cat --output tmp/h_%h c
  $ hg cat --output tmp/r_%r c
  $ hg cat --output tmp/%s_s c
  $ hg cat --output tmp/%d%%_d c
  $ hg cat --output tmp/%p_p c
  $ hg log -r . --template "{rev}: {node|short}\n"
  2: 45116003780e
  $ find tmp -type f | sort
  tmp/.%_d
  tmp/HH_45116003780e3678b333fb2c99fa7d559c8457e9
  tmp/RR_2
  tmp/c_p
  tmp/c_s
  tmp/h_45116003780e
  tmp/r_2

Test working directory

  $ echo b-wdir > b
  $ hg cat -r 'wdir()' b
  b-wdir
