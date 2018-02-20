# Simple sparse linear repository with empty commits to showcase how bisect
# skips them. We mark the latest as bad and the first as good and we expect
# the bisect algorithm to skip the empty commits (2,3) which are visited by
# using the default version
#
#  7: kb (known bad)
#    |
#  6: ec (empty commit)
#    |
#  5: c  (commit)
#    |
#  4: x  (introduce fault)
#    |
#  3: ec (empty commit)
#    |
#  2: ec (empty commit)
#    |
#  1: c  (commit)
#    |
#  0: kg (known good)
#

test bisect-sparse
  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > strip=
  > EOF

  $ echo a > show
  $ echo x > hide
  $ hg ci -Aqm 'known good'

  $ echo a >> show
  $ echo y >> hide
  $ hg ci -Aqm 'on top of good'

  $ echo y >> hide
  $ hg ci -Aqm 'empty sparse 1'

  $ echo y >> hide
  $ hg ci -Aqm 'empty sparse 2'

  $ echo a >> show
  $ echo y >> hide
  $ hg ci -Aqm 'introduce fault'

  $ echo a >> show
  $ echo y >> hide
  $ hg ci -Aqm 'on top of bad'

  $ echo y >> hide
  $ hg ci -Aqm 'empty sparse 1'

  $ echo y >> hide
  $ hg ci -Aqm 'empty sparse 2'

  $ hg sparse --include show

verify bisect skips empty sparse commits (2,3)

  $ hg up -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  $ hg up default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad
  Testing changeset 4:4c3171169989 (7 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad
  Testing changeset 1:4a28797ad698 (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  The first bad revision is:
  changeset:   4:4c3171169989
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     introduce fault
  
