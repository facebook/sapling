#chg-compatible

  $ disable treemanifest
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

Test template output

  $ hg --cwd tmp cat ../b ../c -T '== {path} ({abspath}) ==\n{data}'
  == ../b (b) ==
  1
  == ../c (c) ==
  3

  $ hg cat b c -Tjson --output -
  [
   {
    "abspath": "b",
    "data": "1\n",
    "path": "b"
   },
   {
    "abspath": "c",
    "data": "3\n",
    "path": "c"
   }
  ]

  $ hg cat b c -Tjson --output 'tmp/%p.json'
  $ cat tmp/b.json
  [
   {
    "abspath": "b",
    "data": "1\n",
    "path": "b"
   }
  ]
  $ cat tmp/c.json
  [
   {
    "abspath": "c",
    "data": "3\n",
    "path": "c"
   }
  ]

Test working directory

  $ echo b-wdir > b
  $ hg cat -r 'wdir()' b
  b-wdir

Environment variables are not visible by default

  $ PATTERN='t4' hg log -r '.' -T "{ifcontains('PATTERN', envvars, 'yes', 'no')}\n"
  no

Environment variable visibility can be explicit

  $ PATTERN='t4' hg log -r '.' -T "{envvars % '{key} -> {value}\n'}" \
  >                 --config "experimental.exportableenviron=PATTERN"
  PATTERN -> t4

Test behavior of output when directory structure does not already exist

  $ mkdir foo
  $ echo a > foo/a
  $ hg add foo/a
  $ hg commit -qm "add foo/a"
  $ hg cat --output "output/%p" foo/a
  $ cat output/foo/a
  a
