#debugruntest-compatible
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
  skip 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  skip 112478962961147124edd43549aedd1a335e44bf
  skip 26805aba1e600a82e93661149f2313866a221a7b
  skip f585351a92f85104bff7c284233c338b10eb1df7
  skip a194cadd16930608adaa649035ad4c16930cbd0f
  skip 43195508e3bb704c08d24c40375bdd826789dd72
  skip a31451c3c1debad52cf22ef2aebfc88c75dc899a
  skip 47eb959e86339c47666b6d1e12c7a9ea534aea1c
  skip 08eb06eada62c63c386dde447d379684d2a0156d
  skip fb35f87c67da3431b7514753fc516ec66a60be78
  skip 652ea04869f63d6b6ce47da40f4fb76e01516a98
  skip 71d6430ccd823b5187025bfdedc0ac6d8c0f7e34

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
