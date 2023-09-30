#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ enable morestatus 
  $ setconfig morestatus.show=true
  $ eagerepo
  $ hg init repo
  $ cd repo

# committing changes

  $ drawdag <<'EOS'
  > C
  > :
  > A
  > EOS
  $ hg log -G -T '{node|short} {desc}\n'
  o  26805aba1e60 C
  │
  o  112478962961 B
  │
  o  426bada5c675 A

Test from bad to good

  $ hg bisect -b $A
  $ hg bisect -g $C
  Testing changeset 112478962961 (2 changesets remaining, ~1 tests)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  
  # The repository is in an unfinished *bisect* state.
  # Current bisect state: 1 good commit(s), 1 bad commit(s), 0 skip commit(s)
  # 
  # Current Tracker: bad commit     current        good commit
  #                  426bada5c675...112478962961...26805aba1e60
  # Commits remaining:           2
  # Estimated bisects remaining: 1
  # To mark the commit good:     hg bisect --good
  # To mark the commit bad:      hg bisect --bad
  # To abort:                    hg bisect --reset
  $ hg bisect -r

Test from good to bad
  $ hg bisect -g $A
  $ hg bisect -b $C
  Testing changeset 112478962961 (2 changesets remaining, ~1 tests)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  
  # The repository is in an unfinished *bisect* state.
  # Current bisect state: 1 good commit(s), 1 bad commit(s), 0 skip commit(s)
  # 
  # Current Tracker: good commit    current        bad commit
  #                  426bada5c675...112478962961...26805aba1e60
  # Commits remaining:           2
  # Estimated bisects remaining: 1
  # To mark the commit good:     hg bisect --good
  # To mark the commit bad:      hg bisect --bad
  # To abort:                    hg bisect --reset
  $ hg bisect -r
