# The tests in test-bisect are done on a linear history. Here the
# following repository history is used for testing:
#
#                      17
#                       |
#                18    16
#                  \  /
#                   15
#                  /  \
#                 /    \
#               10     13
#               / \     |
#              /   \    |  14
#         7   6     9  12 /
#          \ / \    |   |/
#           4   \   |  11
#            \   \  |  /
#             3   5 | /
#              \ /  |/
#               2   8
#                \ /
#                 1
#                 |
#                 0

init

  $ hg init

committing changes

  $ echo > a
  $ echo '0' >> a
  $ hg add a
  $ hg ci -m "0" -d "0 0"
  $ echo '1' >> a
  $ hg ci -m "1" -d "1 0"
  $ echo '2' >> a
  $ hg ci -m "2" -d "2 0"
  $ echo '3' >> a
  $ hg ci -m "3" -d "3 0"
  $ echo '4' >> a
  $ hg ci -m "4" -d "4 0"

create branch

  $ hg up -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo '5' >> b
  $ hg add b
  $ hg ci -m "5" -d "5 0"
  created new head

merge

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge 4,5" -d "6 0"

create branch

  $ hg up -r 4
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '7' > c
  $ hg add c
  $ hg ci -m "7" -d "7 0"
  created new head

create branch

  $ hg up -r 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '8' > d
  $ hg add d
  $ hg ci -m "8" -d "8 0"
  created new head
  $ echo '9' >> d
  $ hg ci -m "9" -d "9 0"

merge

  $ hg merge -r 6
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge 6,9" -d "10 0"

create branch

  $ hg up -r 8
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo '11' > e
  $ hg add e
  $ hg ci -m "11" -d "11 0"
  created new head
  $ echo '12' >> e
  $ hg ci -m "12" -d "12 0"
  $ echo '13' >> e
  $ hg ci -m "13" -d "13 0"

create branch

  $ hg up -r 11
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo '14' > f
  $ hg add f
  $ hg ci -m "14" -d "14 0"
  created new head

merge

  $ hg up -r 13 -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge -r 10
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge 10,13" -d "15 0"
  $ echo '16' >> e
  $ hg ci -m "16" -d "16 0"
  $ echo '17' >> e
  $ hg ci -m "17" -d "17 0"

create branch

  $ hg up -r 15
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo '18' >> e
  $ hg ci -m "18" -d "18 0"
  created new head

log

  $ hg log
  changeset:   18:d42e18c7bc9b
  tag:         tip
  parent:      15:857b178a7cf3
  user:        test
  date:        Thu Jan 01 00:00:18 1970 +0000
  summary:     18
  
  changeset:   17:228c06deef46
  user:        test
  date:        Thu Jan 01 00:00:17 1970 +0000
  summary:     17
  
  changeset:   16:609d82a7ebae
  user:        test
  date:        Thu Jan 01 00:00:16 1970 +0000
  summary:     16
  
  changeset:   15:857b178a7cf3
  parent:      13:b0a32c86eb31
  parent:      10:429fcd26f52d
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     merge 10,13
  
  changeset:   14:faa450606157
  parent:      11:82ca6f06eccd
  user:        test
  date:        Thu Jan 01 00:00:14 1970 +0000
  summary:     14
  
  changeset:   13:b0a32c86eb31
  user:        test
  date:        Thu Jan 01 00:00:13 1970 +0000
  summary:     13
  
  changeset:   12:9f259202bbe7
  user:        test
  date:        Thu Jan 01 00:00:12 1970 +0000
  summary:     12
  
  changeset:   11:82ca6f06eccd
  parent:      8:dab8161ac8fc
  user:        test
  date:        Thu Jan 01 00:00:11 1970 +0000
  summary:     11
  
  changeset:   10:429fcd26f52d
  parent:      9:3c77083deb4a
  parent:      6:a214d5d3811a
  user:        test
  date:        Thu Jan 01 00:00:10 1970 +0000
  summary:     merge 6,9
  
  changeset:   9:3c77083deb4a
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     9
  
  changeset:   8:dab8161ac8fc
  parent:      1:4ca5088da217
  user:        test
  date:        Thu Jan 01 00:00:08 1970 +0000
  summary:     8
  
  changeset:   7:50c76098bbf2
  parent:      4:5c668c22234f
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     7
  
  changeset:   6:a214d5d3811a
  parent:      5:385a529b6670
  parent:      4:5c668c22234f
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     merge 4,5
  
  changeset:   5:385a529b6670
  parent:      2:051e12f87bf1
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     5
  
  changeset:   4:5c668c22234f
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     4
  
  changeset:   3:0950834f0a9c
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     3
  
  changeset:   2:051e12f87bf1
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  
  changeset:   1:4ca5088da217
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     1
  
  changeset:   0:33b1f9bc8bc5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  

hg up -C

  $ hg up -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

complex bisect test 1  # first bad rev is 9

  $ hg bisect -r
  $ hg bisect -g 0
  $ hg bisect -b 17   # -> update to rev 6
  Testing changeset 6:a214d5d3811a (15 changesets remaining, ~3 tests)
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  17:228c06deef46
  $ hg log -q -r 'bisect(untested)'
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  $ hg log -q -r 'bisect(ignored)'
  $ hg bisect -g      # -> update to rev 13
  Testing changeset 13:b0a32c86eb31 (9 changesets remaining, ~3 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -s      # -> update to rev 10
  Testing changeset 10:429fcd26f52d (9 changesets remaining, ~3 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -b      # -> update to rev 8
  Testing changeset 8:dab8161ac8fc (3 changesets remaining, ~1 tests)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -g      # -> update to rev 9
  Testing changeset 9:3c77083deb4a (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -b
  The first bad revision is:
  changeset:   9:3c77083deb4a
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     9
  
  $ hg log -q -r 'bisect(range)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b
  $ hg log -q -r 'bisect(untested)'
  11:82ca6f06eccd
  12:9f259202bbe7
  $ hg log -q -r 'bisect(goods)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  $ hg log -q -r 'bisect(bads)'
  9:3c77083deb4a
  10:429fcd26f52d
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b

complex bisect test 2  # first good rev is 13

  $ hg bisect -r
  $ hg bisect -g 18
  $ hg bisect -b 1    # -> update to rev 6
  Testing changeset 6:a214d5d3811a (13 changesets remaining, ~3 tests)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -s      # -> update to rev 10
  Testing changeset 10:429fcd26f52d (13 changesets remaining, ~3 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  6:a214d5d3811a
  18:d42e18c7bc9b
  $ hg bisect -b      # -> update to rev 12
  Testing changeset 12:9f259202bbe7 (5 changesets remaining, ~2 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  18:d42e18c7bc9b
  $ hg log -q -r 'bisect(untested)'
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  $ hg bisect -b      # -> update to rev 13
  Testing changeset 13:b0a32c86eb31 (3 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  The first good revision is:
  changeset:   13:b0a32c86eb31
  user:        test
  date:        Thu Jan 01 00:00:13 1970 +0000
  summary:     13
  
  $ hg log -q -r 'bisect(range)'
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  18:d42e18c7bc9b

complex bisect test 3

first bad rev is 15
10,9,13 are skipped an might be the first bad revisions as well

  $ hg bisect -r
  $ hg bisect -g 1
  $ hg bisect -b 16   # -> update to rev 6
  Testing changeset 6:a214d5d3811a (13 changesets remaining, ~3 tests)
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  16:609d82a7ebae
  17:228c06deef46
  $ hg bisect -g      # -> update to rev 13
  Testing changeset 13:b0a32c86eb31 (8 changesets remaining, ~3 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -s      # -> update to rev 10
  Testing changeset 10:429fcd26f52d (8 changesets remaining, ~3 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -s      # -> update to rev 12
  Testing changeset 12:9f259202bbe7 (8 changesets remaining, ~3 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  10:429fcd26f52d
  13:b0a32c86eb31
  16:609d82a7ebae
  17:228c06deef46
  $ hg bisect -g      # -> update to rev 9
  Testing changeset 9:3c77083deb4a (5 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -s      # -> update to rev 15
  Testing changeset 15:857b178a7cf3 (5 changesets remaining, ~2 tests)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(ignored)'
  $ hg bisect -b
  Due to skipped revisions, the first bad revision could be any of:
  changeset:   9:3c77083deb4a
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     9
  
  changeset:   10:429fcd26f52d
  parent:      9:3c77083deb4a
  parent:      6:a214d5d3811a
  user:        test
  date:        Thu Jan 01 00:00:10 1970 +0000
  summary:     merge 6,9
  
  changeset:   13:b0a32c86eb31
  user:        test
  date:        Thu Jan 01 00:00:13 1970 +0000
  summary:     13
  
  changeset:   15:857b178a7cf3
  parent:      13:b0a32c86eb31
  parent:      10:429fcd26f52d
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     merge 10,13
  
  $ hg log -q -r 'bisect(range)'
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  $ hg log -q -r 'bisect(ignored)'

complex bisect test 4

first good revision is 17
15,16 are skipped an might be the first good revisions as well

  $ hg bisect -r
  $ hg bisect -g 17
  $ hg bisect -b 8    # -> update to rev 10
  Testing changeset 13:b0a32c86eb31 (8 changesets remaining, ~3 tests)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -b      # -> update to rev 13
  Testing changeset 10:429fcd26f52d (5 changesets remaining, ~2 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bisect -b      # -> update to rev 15
  Testing changeset 15:857b178a7cf3 (3 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  17:228c06deef46
  $ hg bisect -s      # -> update to rev 16
  Testing changeset 16:609d82a7ebae (3 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  17:228c06deef46
  $ hg bisect -s
  Due to skipped revisions, the first good revision could be any of:
  changeset:   15:857b178a7cf3
  parent:      13:b0a32c86eb31
  parent:      10:429fcd26f52d
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     merge 10,13
  
  changeset:   16:609d82a7ebae
  user:        test
  date:        Thu Jan 01 00:00:16 1970 +0000
  summary:     16
  
  changeset:   17:228c06deef46
  user:        test
  date:        Thu Jan 01 00:00:17 1970 +0000
  summary:     17
  
  $ hg log -q -r 'bisect(range)'
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46

test unrelated revs:

  $ hg bisect --reset
  $ hg bisect -b 7
  $ hg bisect -g 14
  abort: starting revisions are not directly related
  [255]
  $ hg log -q -r 'bisect(range)'
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  7:50c76098bbf2
  14:faa450606157
  $ hg bisect --reset

end at merge: 17 bad, 11 good (but 9 is first bad)

  $ hg bisect -r
  $ hg bisect -b 17
  $ hg bisect -g 11
  Testing changeset 13:b0a32c86eb31 (5 changesets remaining, ~2 tests)
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(ignored)'
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  9:3c77083deb4a
  10:429fcd26f52d
  $ hg bisect -g
  Testing changeset 15:857b178a7cf3 (3 changesets remaining, ~1 tests)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -b
  The first bad revision is:
  changeset:   15:857b178a7cf3
  parent:      13:b0a32c86eb31
  parent:      10:429fcd26f52d
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     merge 10,13
  
  Not all ancestors of this changeset have been checked.
  Use bisect --extend to continue the bisection from
  the common ancestor, dab8161ac8fc.
  $ hg log -q -r 'bisect(range)'
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  8:dab8161ac8fc
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b
  $ hg log -q -r 'bisect(untested)'
  $ hg log -q -r 'bisect(ignored)'
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  9:3c77083deb4a
  10:429fcd26f52d
  $ hg bisect --extend
  Extending search to changeset 8:dab8161ac8fc
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(untested)'
  $ hg log -q -r 'bisect(ignored)'
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  9:3c77083deb4a
  10:429fcd26f52d
  $ hg bisect -g # dab8161ac8fc
  Testing changeset 9:3c77083deb4a (3 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(untested)'
  9:3c77083deb4a
  10:429fcd26f52d
  $ hg log -q -r 'bisect(ignored)'
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  $ hg log -q -r 'bisect(goods)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  8:dab8161ac8fc
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  $ hg log -q -r 'bisect(bads)'
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b
  $ hg bisect -b
  The first bad revision is:
  changeset:   9:3c77083deb4a
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     9
  
  $ hg log -q -r 'bisect(range)'
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  8:dab8161ac8fc
  9:3c77083deb4a
  10:429fcd26f52d
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b
  $ hg log -q -r 'bisect(untested)'
  $ hg log -q -r 'bisect(ignored)'
  2:051e12f87bf1
  3:0950834f0a9c
  4:5c668c22234f
  5:385a529b6670
  6:a214d5d3811a
  $ hg log -q -r 'bisect(goods)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  8:dab8161ac8fc
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  $ hg log -q -r 'bisect(bads)'
  9:3c77083deb4a
  10:429fcd26f52d
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b

user adds irrelevant but consistent information (here: -g 2) to bisect state

  $ hg bisect -r
  $ hg bisect -b 13
  $ hg bisect -g 8
  Testing changeset 11:82ca6f06eccd (3 changesets remaining, ~1 tests)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(untested)'
  11:82ca6f06eccd
  12:9f259202bbe7
  $ hg bisect -g 2
  Testing changeset 11:82ca6f06eccd (3 changesets remaining, ~1 tests)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -q -r 'bisect(untested)'
  11:82ca6f06eccd
  12:9f259202bbe7
  $ hg bisect -b
  The first bad revision is:
  changeset:   11:82ca6f06eccd
  parent:      8:dab8161ac8fc
  user:        test
  date:        Thu Jan 01 00:00:11 1970 +0000
  summary:     11
  
  $ hg log -q -r 'bisect(range)'
  8:dab8161ac8fc
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  $ hg log -q -r 'bisect(pruned)'
  0:33b1f9bc8bc5
  1:4ca5088da217
  2:051e12f87bf1
  8:dab8161ac8fc
  11:82ca6f06eccd
  12:9f259202bbe7
  13:b0a32c86eb31
  14:faa450606157
  15:857b178a7cf3
  16:609d82a7ebae
  17:228c06deef46
  18:d42e18c7bc9b
  $ hg log -q -r 'bisect(untested)'
