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

  $ eagerepo
  $ enable sparse amend
  $ setconfig clone.use-rust=true

test bisect-sparse
  $ hg init server
  $ cd server

  $ drawdag <<EOS
  > J  # J/sparse-excluded-file = y
  > |
  > I  # I/sparse-excluded-file = x
  > |
  > H  # H/sparse-excluded-file = y
  > |
  > G  # G/sparse-included-file = a
  > |  # G/sparse-excluded-file = x
  > |
  > F  # F/sparse-included-file = b
  > |  # F/sparse-excluded-file = y
  > |
  > E  # E/sparse-excluded-file = x
  > |
  > D  # D/sparse-excluded-file = y
  > |
  > C  # C/sparse-excluded-file = x
  > |
  > B  # B/sparse-excluded-file = y
  > |
  > A  # A/sparse-included-file = a
  >    # A/sparse-excluded-file = x
  >    # A/profile = profile\nsparse-included-file\n
  > python:
  > commit('G', 'introducing bug')
  > EOS

  $ cd
  $ hg clone -q test:server client --enable-profile profile
  $ cd client

verify bisect skips empty sparse commits (2,3)

  $ hg up -r $A
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  $ hg up $J
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg bisect --bad
  Skipping changeset 61165d92eeb6 as there are no changes inside
  the sparse profile from the known good changeset 67d16e36726d
  Skipping changeset b81af7b7acae as there are no changes inside
  the sparse profile from the known bad changeset 96593dec1c75
  Testing changeset cb60aec397f6 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  The first bad revision is:
  commit:      b81af7b7acae
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     introducing bug

check --nosparseskip flag

  $ hg bisect --reset
  $ hg bisect -g $A
  $ hg bisect -b $J -S
  Testing changeset 61165d92eeb6 (9 changesets remaining, ~3 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good --nosparseskip
  Testing changeset b81af7b7acae (5 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad --nosparseskip
  Testing changeset cb60aec397f6 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  The first bad revision is:
  commit:      b81af7b7acae
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     introducing bug


verify skipping works with --command flag

  $ cat > script.py <<EOF
  > from __future__ import absolute_import
  > import sys
  > from sapling import hg, node, ui as uimod
  > repo = hg.repository(uimod.ui.load(), '.')
  > if repo.changelog.isancestor(node.bin("$G"), repo['.'].node()): # where the bug was introduced
  >     sys.exit(1)
  > EOF
  $ chmod +x script.py

  $ hg bisect --reset
  $ hg bisect -g $A
  $ hg up $J
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --command "hg debugpython -- script.py"
  changeset 96593dec1c75: bad
  Skipping changeset 61165d92eeb6 as there are no changes inside
  the sparse profile from the known good changeset 67d16e36726d
  Skipping changeset b81af7b7acae as there are no changes inside
  the sparse profile from the known bad changeset 96593dec1c75
  Testing changeset cb60aec397f6 (2 changesets remaining, ~1 tests)
  changeset cb60aec397f6: good
  Testing changeset b81af7b7acae (0 changesets remaining, ~0 tests)
  The first bad revision is:
  commit:      b81af7b7acae
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     introducing bug




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
  $ hg up $H
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

  $ hg merge -r $J
  temporarily included 1 file(s) in the sparse checkout for merging
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -Aqm '13: merge(9,12)'

  $ echo t > sparse-included-file
  $ echo v > sparse-excluded-file
  $ hg ci -Aqm '14'

  $ hg bisect -g $I
  $ hg bisect -b 'desc(14)'
  Skipping changeset 96593dec1c75 as there are no changes inside
  the sparse profile from the known good changeset a1deef3f19b6
  Testing changeset 5208d98c5d2e (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --bad
  The first bad revision is:
  commit:      5208d98c5d2e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     13: merge(9,12)
  
  Not all ancestors of this changeset have been checked.
  Use bisect --extend to continue the bisection from
  the common ancestor, bef5da0179e1.





  $ hg bisect --extend
  Extending search to changeset bef5da0179e1
  Skipping changeset bef5da0179e1 as there are no changes inside
  the sparse profile from the known good changeset 96593dec1c75
  Testing changeset 9351b91f8f7a (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect --good
  Skipping changeset 8a99ef081954 as there are no changes inside
  the sparse profile from the known bad changeset 5208d98c5d2e
  The first bad revision is:
  commit:      8a99ef081954
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
  $ echo sparse-new-excluded-file >> profile
  $ hg ci -Aqm 'known good - 15'

  $ echo "empty good" > sparse-new-excluded-file
  $ hg ci -Aqm 'empty good - 16'

  $ echo "empty bad" > sparse-new-excluded-file
  $ echo "empty bad" > sparse-included-file
  $ hg ci -Aqm 'empty bad - 17'

  $ echo "known bad" > sparse-new-excluded-file
  $ hg ci -Aqm 'known bad - 18'

  $ sed -i '/sparse-new-excluded-file/d' profile
  $ hg amend --to "desc('known good - 15')"

  $ hg bisect --reset
  $ hg bisect -g "desc('known good - 15')"
  $ hg bisect -c "test $(hg log -r . -T '{rev}') -lt 17"
  changeset 03845d757c47: bad
  Skipping changeset 3fd59de51436 as there are no changes inside
  the sparse profile from the known good changeset 8f072b3c6011
  Skipping changeset 0ed3490f5393 as there are no changes inside
  the sparse profile from the known bad changeset 03845d757c47
  The first bad revision is:
  commit:      0ed3490f5393
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     empty bad - 17
