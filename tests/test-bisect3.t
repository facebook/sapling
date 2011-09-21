# Here we create a simple DAG which has just enough of the required
# topology to test all the bisection status labels:
#
#           13--14
#          /
#   0--1--2--3---------9--10--11--12
#       \             /
#        4--5--6--7--8


  $ hg init

  $ echo '0' >a
  $ hg add a
  $ hg ci -u test -d '0 0' -m '0'
  $ echo '1' >a
  $ hg ci -u test -d '0 1' -m '1'

branch 2-3

  $ echo '2' >b
  $ hg add b
  $ hg ci -u test -d '0 2' -m '2'
  $ echo '3' >b
  $ hg ci -u test -d '0 3' -m '3'

branch 4-8

  $ hg up -r 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '4' >c
  $ hg add c
  $ hg ci -u test -d '0 4' -m '4'
  created new head
  $ echo '5' >c
  $ hg ci -u test -d '0 5' -m '5'
  $ echo '6' >c
  $ hg ci -u test -d '0 6' -m '6'
  $ echo '7' >c
  $ hg ci -u test -d '0 7' -m '7'
  $ echo '8' >c
  $ hg ci -u test -d '0 8' -m '8'

merge

  $ hg merge -r 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -u test -d '0 9' -m '9=8+3'

  $ echo '10' >a
  $ hg ci -u test -d '0 10' -m '10'
  $ echo '11' >a
  $ hg ci -u test -d '0 11' -m '11'
  $ echo '12' >a
  $ hg ci -u test -d '0 12' -m '12'

unrelated branch

  $ hg up -r 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '13' >d
  $ hg add d
  $ hg ci -u test -d '0 13' -m '13'
  created new head
  $ echo '14' >d
  $ hg ci -u test -d '0 14' -m '14'

mark changesets

  $ hg bisect --reset
  $ hg bisect --good 4
  $ hg bisect --good 6
  $ hg bisect --bad 12
  Testing changeset 9:8bcbdb072033 (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect --bad 10
  Testing changeset 8:3cd112f87d77 (4 changesets remaining, ~2 tests)
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect --skip 7
  Testing changeset 8:3cd112f87d77 (4 changesets remaining, ~2 tests)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

test template

  $ hg log --template '{rev}:{node|short} {bisect}\n'
  14:cecd84203acc 
  13:86f7c8cdb6df 
  12:a76089b5f47c bad
  11:5c3eb122d29c bad (implicit)
  10:b097cef2be03 bad
  9:8bcbdb072033 untested
  8:3cd112f87d77 untested
  7:577e237a73bd skipped
  6:e597fa2707c5 good
  5:b9cea37a76bc good (implicit)
  4:da6b357259d7 good
  3:e7f031aee8ca ignored
  2:b1ad1b6bcc5c ignored
  1:37f42ae8b45e good (implicit)
  0:b4e73ffab476 good (implicit)
  $ hg log --template '{bisect|shortbisect} {rev}:{node|short}\n'
    14:cecd84203acc
    13:86f7c8cdb6df
  B 12:a76089b5f47c
  B 11:5c3eb122d29c
  B 10:b097cef2be03
  U 9:8bcbdb072033
  U 8:3cd112f87d77
  S 7:577e237a73bd
  G 6:e597fa2707c5
  G 5:b9cea37a76bc
  G 4:da6b357259d7
  I 3:e7f031aee8ca
  I 2:b1ad1b6bcc5c
  G 1:37f42ae8b45e
  G 0:b4e73ffab476

test style

  $ hg log --style bisect
  changeset:   14:cecd84203acc
  bisect:      
  tag:         tip
  user:        test
  date:        Wed Dec 31 23:59:46 1969 -0000
  summary:     14
  
  changeset:   13:86f7c8cdb6df
  bisect:      
  parent:      3:e7f031aee8ca
  user:        test
  date:        Wed Dec 31 23:59:47 1969 -0000
  summary:     13
  
  changeset:   12:a76089b5f47c
  bisect:      bad
  user:        test
  date:        Wed Dec 31 23:59:48 1969 -0000
  summary:     12
  
  changeset:   11:5c3eb122d29c
  bisect:      bad (implicit)
  user:        test
  date:        Wed Dec 31 23:59:49 1969 -0000
  summary:     11
  
  changeset:   10:b097cef2be03
  bisect:      bad
  user:        test
  date:        Wed Dec 31 23:59:50 1969 -0000
  summary:     10
  
  changeset:   9:8bcbdb072033
  bisect:      untested
  parent:      8:3cd112f87d77
  parent:      3:e7f031aee8ca
  user:        test
  date:        Wed Dec 31 23:59:51 1969 -0000
  summary:     9=8+3
  
  changeset:   8:3cd112f87d77
  bisect:      untested
  user:        test
  date:        Wed Dec 31 23:59:52 1969 -0000
  summary:     8
  
  changeset:   7:577e237a73bd
  bisect:      skipped
  user:        test
  date:        Wed Dec 31 23:59:53 1969 -0000
  summary:     7
  
  changeset:   6:e597fa2707c5
  bisect:      good
  user:        test
  date:        Wed Dec 31 23:59:54 1969 -0000
  summary:     6
  
  changeset:   5:b9cea37a76bc
  bisect:      good (implicit)
  user:        test
  date:        Wed Dec 31 23:59:55 1969 -0000
  summary:     5
  
  changeset:   4:da6b357259d7
  bisect:      good
  parent:      1:37f42ae8b45e
  user:        test
  date:        Wed Dec 31 23:59:56 1969 -0000
  summary:     4
  
  changeset:   3:e7f031aee8ca
  bisect:      ignored
  user:        test
  date:        Wed Dec 31 23:59:57 1969 -0000
  summary:     3
  
  changeset:   2:b1ad1b6bcc5c
  bisect:      ignored
  user:        test
  date:        Wed Dec 31 23:59:58 1969 -0000
  summary:     2
  
  changeset:   1:37f42ae8b45e
  bisect:      good (implicit)
  user:        test
  date:        Wed Dec 31 23:59:59 1969 -0000
  summary:     1
  
  changeset:   0:b4e73ffab476
  bisect:      good (implicit)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  $ hg log --quiet --style bisect
    14:cecd84203acc
    13:86f7c8cdb6df
  B 12:a76089b5f47c
  B 11:5c3eb122d29c
  B 10:b097cef2be03
  U 9:8bcbdb072033
  U 8:3cd112f87d77
  S 7:577e237a73bd
  G 6:e597fa2707c5
  G 5:b9cea37a76bc
  G 4:da6b357259d7
  I 3:e7f031aee8ca
  I 2:b1ad1b6bcc5c
  G 1:37f42ae8b45e
  G 0:b4e73ffab476
