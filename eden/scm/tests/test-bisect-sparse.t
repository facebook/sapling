#chg-compatible

#  Linear tree case
#
#  9 <- known bad - - -
#  |                   |
#  8                   |
#  |                 nothing
#  7                 changes
#  |                   |
#  6 <- introduce bug -       <- 1 iter: skipping as bad, 2: the answer
#  |
#  5                          <- 2 iter: checking this
#  |
#  4 - - - - - - - - -        <- 1 iter: skipping as good
#  |                  |
#  3                  |
#  |                nothig
#  2              changes in
#  |                sparse
#  1                  |
#  |                  |
#  0 <- known good - -

test bisect-sparse
  $ enable sparse
  $ hg init myrepo
  $ cd myrepo

  $ echo a > sparse-included-file
  $ echo x > sparse-excluded-file
  $ hg ci -Aqm 'good 0'

  $ echo y > sparse-excluded-file
  $ hg ci -Aqm 'good 1'

  $ echo x > sparse-excluded-file
  $ hg ci -Aqm 'good 2'

  $ echo y > sparse-excluded-file
  $ hg ci -Aqm 'good 3'

  $ echo x > sparse-excluded-file
  $ hg ci -Aqm 'good 4'

  $ echo b > sparse-included-file
  $ echo y > sparse-excluded-file
  $ hg ci -Aqm 'good 5'

  $ echo a > sparse-included-file
  $ echo x > sparse-excluded-file
  $ hg ci -Aqm 'bad  6 - introducing bug'

  $ echo y > sparse-excluded-file
  $ hg ci -Aqm 'bad  7'

  $ echo x > sparse-excluded-file
  $ hg ci -Aqm 'bad  8'

  $ echo y > sparse-excluded-file
  $ hg ci -Aqm 'bad  9'

  $ hg sparse include sparse-included-file
  $ hg sparse exclude sparse-excluded-file

verify bisect skips empty sparse commits (2,3)

  $ hg up -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  $ hg up 9
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad
  Skipping changeset 4:e116419d642b as there are no changes inside
  the sparse profile from the known good changeset 0:a75e20cc7b2a
  Skipping changeset 6:6b9461e31152 as there are no changes inside
  the sparse profile from the known bad changeset 9:d910e57b873b
  Testing changeset 2ecc2db0df15 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  The first bad revision is:
  changeset:   6:6b9461e31152
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     bad  6 - introducing bug
  

check --nosparseskip flag

  $ hg bisect --reset
  $ hg bisect -g 0
  $ hg bisect -b 9 -S
  Testing changeset e116419d642b (9 changesets remaining, ~3 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good --nosparseskip
  Testing changeset 6b9461e31152 (5 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad --nosparseskip
  Testing changeset 2ecc2db0df15 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  The first bad revision is:
  changeset:   6:6b9461e31152
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     bad  6 - introducing bug
  


verify skipping works with --command flag

  $ cat > script.py <<EOF
  > from __future__ import absolute_import
  > import sys
  > from edenscm.mercurial import hg, ui as uimod
  > repo = hg.repository(uimod.ui.load(), '.')
  > if repo['.'].rev() >= 6: # where the bug was introduced
  >     sys.exit(1)
  > EOF
  $ chmod +x script.py

  $ hg bisect --reset
  $ hg bisect -g 0
  $ hg up 9
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --command "hg debugpython -- script.py"
  changeset 9:d910e57b873b: bad
  Skipping changeset 4:e116419d642b as there are no changes inside
  the sparse profile from the known good changeset 0:a75e20cc7b2a
  Skipping changeset 6:6b9461e31152 as there are no changes inside
  the sparse profile from the known bad changeset 9:d910e57b873b
  changeset 5:2ecc2db0df15: good
  The first bad revision is:
  changeset:   6:6b9461e31152
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     bad  6 - introducing bug
  




# Tree with merge commits case
#
#                  14 <- known bad
#                  |
#                  13 - - - - - - - - - - nothing
#                 /  \                    changes
#  nothing       |    12 <- introduce - - - -|
#  changes - - - 9    |        bug
#  |             |    11
#  |             |    |
#  |    known -> 8    10
#  |    good      \  /
#  |               7 <- extends to
#  |               |
#   - - - - - - - -6
#                  |

New test set

  $ hg bisect --reset
  $ hg up 7
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo r > sparse-included-file
  $ echo z > sparse-excluded-file
  $ echo dsf > esFE
  $ hg ci -Am '10'

  $ echo a > sparse-included-file
  $ echo x > sparse-excluded-file
  $ hg ci -Aqm '11'

  $ echo r > sparse-included-file
  $ echo z > sparse-excluded-file
  $ hg ci -Aqm '12'

  $ hg merge -r 9
  temporarily included 1 file(s) in the sparse checkout for merging
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -Aqm '13: merge(9,12)'

  $ echo t > sparse-included-file
  $ echo v > sparse-excluded-file
  $ hg ci -Aqm '14'

  $ hg bisect -g 8
  $ hg bisect -b 14
  Skipping changeset 9:d910e57b873b as there are no changes inside
  the sparse profile from the known good changeset 8:a6b1a23ad56a
  Testing changeset a41c9f2666a8 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad
  The first bad revision is:
  changeset:   13:a41c9f2666a8
  parent:      12:e694d9484bb8
  parent:      9:d910e57b873b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     13: merge(9,12)
  
  Not all ancestors of this changeset have been checked.
  Use bisect --extend to continue the bisection from
  the common ancestor, 94c6ab768eff.





  $ hg bisect --extend
  Extending search to changeset 7:94c6ab768eff
  Skipping changeset 7:94c6ab768eff as there are no changes inside
  the sparse profile from the known good changeset 9:d910e57b873b
  Testing changeset 7038c7a4f757 (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  Skipping changeset 12:e694d9484bb8 as there are no changes inside
  the sparse profile from the known bad changeset 13:a41c9f2666a8
  The first bad revision is:
  changeset:   12:e694d9484bb8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     12
  






Empty case with --command flag: all commits are skipped

#  18 <- known bad - - -
#  |                   |
#  |                 nothing
#  |                 changes
#  |                   |
#  17 <- introduce bug -       <- 1 iter: skipping as bad
#  |
#  16 - - - - - - - - -        <- 1 iter: skipping as good
#  |                  |
#  |                nothing
#  |                changes
#  |                  |
#  15 <- known good - -
#  |

  $ echo "known good" > sparse-new-excluded-file
  $ echo "known good" > sparse-included-file
  $ hg sparse include sparse-new-excluded-file
  $ hg ci -Aqm 'known good - 15'

  $ echo "empty good" > sparse-new-excluded-file
  $ hg ci -Aqm 'empty good - 16'

  $ echo "empty bad" > sparse-new-excluded-file
  $ echo "empty bad" > sparse-included-file
  $ hg ci -Aqm 'empty bad - 17'

  $ echo "known bad" > sparse-new-excluded-file
  $ hg ci -Aqm 'known bad - 18'

  $ hg sparse exclude sparse-new-excluded-file

  $ hg bisect --reset
  $ hg bisect -g 15
  $ hg bisect -c "test $(hg log -r . -T '{rev}') -lt 17"
  changeset 18:ddea298cfd5a: bad
  Skipping changeset 16:8654dd939818 as there are no changes inside
  the sparse profile from the known good changeset 15:6e74f05c0613
  Skipping changeset 17:9ca8d13c5161 as there are no changes inside
  the sparse profile from the known bad changeset 18:ddea298cfd5a
  The first bad revision is:
  changeset:   17:9ca8d13c5161
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     empty bad - 17
  




