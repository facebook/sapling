#chg-compatible
#debugruntest-compatible

# Here we create a simple DAG which has just enough of the required
# topology to test all the bisection status labels:
#
#           13--14
#          /
#   0--1--2--3---------9--10--11--12
#       \             /
#        4--5--6--7--8


  $ hg init repo
  $ cd repo

  $ echo '0' >a
  $ hg add a
  $ hg ci -u test -d '0 0' -m '0'
  $ echo '1' >a
  $ hg ci -u test -d '1 0' -m '1'

branch 2-3

  $ echo '2' >b
  $ hg add b
  $ hg ci -u test -d '2 0' -m '2'
  $ echo '3' >b
  $ hg ci -u test -d '3 0' -m '3'

branch 4-8

  $ hg up -r 'desc(1)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '4' >c
  $ hg add c
  $ hg ci -u test -d '4 0' -m '4'
  $ echo '5' >c
  $ hg ci -u test -d '5 0' -m '5'
  $ echo '6' >c
  $ hg ci -u test -d '6 0' -m '6'
  $ echo '7' >c
  $ hg ci -u test -d '7 0' -m '7'
  $ echo '8' >c
  $ hg ci -u test -d '8 0' -m '8'

merge

  $ hg merge -r 'desc(3)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -u test -d '9 0' -m '9=8+3'

  $ echo '10' >a
  $ hg ci -u test -d '10 0' -m '10'
  $ echo '11' >a
  $ hg ci -u test -d '11 0' -m '11'
  $ echo '12' >a
  $ hg ci -u test -d '12 0' -m '12'

unrelated branch

  $ hg up -r 8417d459b90c8ff7a70033ab503fab3b1524a8ed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '13' >d
  $ hg add d
  $ hg ci -u test -d '13 0' -m '13'
  $ echo '14' >d
  $ hg ci -u test -d '14 0' -m '14'

mark changesets

  $ hg bisect --reset
  $ hg bisect --good 2a1daef14cd4f3d2dd2ca4d90fc67561ed148a24
  $ hg bisect --good 'desc(6)'
  $ hg bisect --bad 'desc(12)'
  Testing changeset 2197c557e14c (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect --bad 'desc(10)'
  Testing changeset e74a86251f58 (4 changesets remaining, ~2 tests)
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect --skip 'desc(7)'
  Testing changeset e74a86251f58 (4 changesets remaining, ~2 tests)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

test template

  $ hg log --template '{node|short} {bisect}\n'
  cbf2f3105bbf 
  e07efca37c43 
  98c6b56349c0 bad
  03f491376e63 bad (implicit)
  c012b15e2409 bad
  2197c557e14c untested
  e74a86251f58 untested
  a5f87041c899 skipped
  7d997bedcd8d good
  2dd1875f1028 good (implicit)
  2a1daef14cd4 good
  8417d459b90c ignored
  e1355ee1f23e ignored
  ce7c85e06a9f good (implicit)
  b4e73ffab476 good (implicit)
  $ hg log --template '{bisect|shortbisect} {node|short}\n'
    cbf2f3105bbf
    e07efca37c43
  B 98c6b56349c0
  B 03f491376e63
  B c012b15e2409
  U 2197c557e14c
  U e74a86251f58
  S a5f87041c899
  G 7d997bedcd8d
  G 2dd1875f1028
  G 2a1daef14cd4
  I 8417d459b90c
  I e1355ee1f23e
  G ce7c85e06a9f
  G b4e73ffab476

test style

  $ hg log --style bisect
  commit:      cbf2f3105bbf
  bisect:      
  user:        test
  date:        Thu Jan 01 00:00:14 1970 +0000
  summary:     14
  
  commit:      e07efca37c43
  bisect:      
  user:        test
  date:        Thu Jan 01 00:00:13 1970 +0000
  summary:     13
  
  commit:      98c6b56349c0
  bisect:      bad
  user:        test
  date:        Thu Jan 01 00:00:12 1970 +0000
  summary:     12
  
  commit:      03f491376e63
  bisect:      bad (implicit)
  user:        test
  date:        Thu Jan 01 00:00:11 1970 +0000
  summary:     11
  
  commit:      c012b15e2409
  bisect:      bad
  user:        test
  date:        Thu Jan 01 00:00:10 1970 +0000
  summary:     10
  
  commit:      2197c557e14c
  bisect:      untested
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     9=8+3
  
  commit:      e74a86251f58
  bisect:      untested
  user:        test
  date:        Thu Jan 01 00:00:08 1970 +0000
  summary:     8
  
  commit:      a5f87041c899
  bisect:      skipped
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     7
  
  commit:      7d997bedcd8d
  bisect:      good
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     6
  
  commit:      2dd1875f1028
  bisect:      good (implicit)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     5
  
  commit:      2a1daef14cd4
  bisect:      good
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     4
  
  commit:      8417d459b90c
  bisect:      ignored
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     3
  
  commit:      e1355ee1f23e
  bisect:      ignored
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  
  commit:      ce7c85e06a9f
  bisect:      good (implicit)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     1
  
  commit:      b4e73ffab476
  bisect:      good (implicit)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  $ hg log --quiet --style bisect
    cbf2f3105bbf
    e07efca37c43
  B 98c6b56349c0
  B 03f491376e63
  B c012b15e2409
  U 2197c557e14c
  U e74a86251f58
  S a5f87041c899
  G 7d997bedcd8d
  G 2dd1875f1028
  G 2a1daef14cd4
  I 8417d459b90c
  I e1355ee1f23e
  G ce7c85e06a9f
  G b4e73ffab476

  $ hg --config extensions.color= --color=debug log --quiet --style bisect
  [log.bisect| ] cbf2f3105bbf
  [log.bisect| ] e07efca37c43
  [log.bisect bisect.bad|B] 98c6b56349c0
  [log.bisect bisect.bad|B] 03f491376e63
  [log.bisect bisect.bad|B] c012b15e2409
  [log.bisect bisect.untested|U] 2197c557e14c
  [log.bisect bisect.untested|U] e74a86251f58
  [log.bisect bisect.skipped|S] a5f87041c899
  [log.bisect bisect.good|G] 7d997bedcd8d
  [log.bisect bisect.good|G] 2dd1875f1028
  [log.bisect bisect.good|G] 2a1daef14cd4
  [log.bisect bisect.ignored|I] 8417d459b90c
  [log.bisect bisect.ignored|I] e1355ee1f23e
  [log.bisect bisect.good|G] ce7c85e06a9f
  [log.bisect bisect.good|G] b4e73ffab476
