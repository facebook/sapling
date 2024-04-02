#debugruntest-compatible

#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ eagerepo
  $ hg init repo
  $ cd repo

# committing changes

  $ drawdag <<'EOS'
  > N
  > :
  > A
  > EOS

  $ hg bisect -s "! (file('path:E') or file('path:M'))"
  $ cat .hg/bisect.state
  skip revset:! (file('path:E') or file('path:M'))

  $ hg bisect -g $A
  $ hg bisect -b $N
  Testing changeset 9bc730a19041 (13 changesets remaining, ~3 tests)
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -b
  Due to skipped revisions, the first bad revision could be any of:
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
  commit:      26805aba1e60
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     C
  
  commit:      f585351a92f8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     D
  
  commit:      9bc730a19041
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     E
