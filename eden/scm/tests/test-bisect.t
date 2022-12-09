#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init repo
  $ cd repo

# committing changes

  $ echo >> a
  $ for i in `seq 0 31`; do
  >   echo a >> a
  >   hg ci -m "msg $i" -d "$i 0" -A a -q
  > done

  $ hg log
  commit:      58c80a7c8a40
  user:        test
  date:        Thu Jan 01 00:00:31 1970 +0000
  summary:     msg 31
  
  commit:      ed2d2f24b11c
  user:        test
  date:        Thu Jan 01 00:00:30 1970 +0000
  summary:     msg 30
  
  commit:      b5bd63375ab9
  user:        test
  date:        Thu Jan 01 00:00:29 1970 +0000
  summary:     msg 29
  
  commit:      8e0c2264c8af
  user:        test
  date:        Thu Jan 01 00:00:28 1970 +0000
  summary:     msg 28
  
  commit:      288867a866e9
  user:        test
  date:        Thu Jan 01 00:00:27 1970 +0000
  summary:     msg 27
  
  commit:      3efc6fd51aeb
  user:        test
  date:        Thu Jan 01 00:00:26 1970 +0000
  summary:     msg 26
  
  commit:      02a84173a97a
  user:        test
  date:        Thu Jan 01 00:00:25 1970 +0000
  summary:     msg 25
  
  commit:      10e0acd3809e
  user:        test
  date:        Thu Jan 01 00:00:24 1970 +0000
  summary:     msg 24
  
  commit:      5ec79163bff4
  user:        test
  date:        Thu Jan 01 00:00:23 1970 +0000
  summary:     msg 23
  
  commit:      06c7993750ce
  user:        test
  date:        Thu Jan 01 00:00:22 1970 +0000
  summary:     msg 22
  
  commit:      e5db6aa3fe2a
  user:        test
  date:        Thu Jan 01 00:00:21 1970 +0000
  summary:     msg 21
  
  commit:      7128fb4fdbc9
  user:        test
  date:        Thu Jan 01 00:00:20 1970 +0000
  summary:     msg 20
  
  commit:      52798545b482
  user:        test
  date:        Thu Jan 01 00:00:19 1970 +0000
  summary:     msg 19
  
  commit:      86977a90077e
  user:        test
  date:        Thu Jan 01 00:00:18 1970 +0000
  summary:     msg 18
  
  commit:      03515f4a9080
  user:        test
  date:        Thu Jan 01 00:00:17 1970 +0000
  summary:     msg 17
  
  commit:      a2e6ea4973e9
  user:        test
  date:        Thu Jan 01 00:00:16 1970 +0000
  summary:     msg 16
  
  commit:      e7fa0811edb0
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     msg 15
  
  commit:      ce8f0998e922
  user:        test
  date:        Thu Jan 01 00:00:14 1970 +0000
  summary:     msg 14
  
  commit:      9d7d07bc967c
  user:        test
  date:        Thu Jan 01 00:00:13 1970 +0000
  summary:     msg 13
  
  commit:      1941b52820a5
  user:        test
  date:        Thu Jan 01 00:00:12 1970 +0000
  summary:     msg 12
  
  commit:      7b4cd9578619
  user:        test
  date:        Thu Jan 01 00:00:11 1970 +0000
  summary:     msg 11
  
  commit:      7c5eff49a6b6
  user:        test
  date:        Thu Jan 01 00:00:10 1970 +0000
  summary:     msg 10
  
  commit:      eb44510ef29a
  user:        test
  date:        Thu Jan 01 00:00:09 1970 +0000
  summary:     msg 9
  
  commit:      453eb4dba229
  user:        test
  date:        Thu Jan 01 00:00:08 1970 +0000
  summary:     msg 8
  
  commit:      03750880c6b5
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     msg 7
  
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6
  
  commit:      7874a09ea728
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     msg 5
  
  commit:      9b2ba8336a65
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     msg 4
  
  commit:      b53bea5e2fcb
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     msg 3
  
  commit:      db07c04beaca
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     msg 2
  
  commit:      5cd978ea5149
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     msg 1
  
  commit:      b99c7b9c8e11
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     msg 0

  $ hg up -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

# bisect test

  $ hg bisect -r
  $ hg bisect -b
  $ hg status -v
  # The repository is in an unfinished *bisect* state.
  
  # To mark the changeset good:    hg bisect --good
  # To mark the changeset bad:     hg bisect --bad
  # To abort:                      hg bisect --reset
  $ hg status -v --config 'commands.status.skipstates=bisect'
  $ hg summary
  parent: 58c80a7c8a40 
   msg 31
  commit: (clean)
  phases: 32 draft
  $ hg bisect -g 1
  Testing changeset a2e6ea4973e9 (30 changesets remaining, ~4 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset 5ec79163bff4 (15 changesets remaining, ~3 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# skip

  $ hg bisect -s
  Testing changeset 10e0acd3809e (15 changesets remaining, ~3 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset 288867a866e9 (7 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset b5bd63375ab9 (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -b
  Testing changeset 8e0c2264c8af (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  The first bad revision is:
  commit:      b5bd63375ab9
  user:        test
  date:        Thu Jan 01 00:00:29 1970 +0000
  summary:     msg 29

# mark revsets instead of single revs

  $ hg bisect -r
  $ hg bisect -b '0::3'
  $ hg bisect -s '13::16'
  $ hg bisect -g '26::tip'
  Testing changeset 1941b52820a5 (23 changesets remaining, ~4 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat .hg/bisect.state
  bad b99c7b9c8e11558adef3fad9af211c58d46f325b
  bad 5cd978ea51499179507ee7b6f340d2dbaa401185
  bad db07c04beaca44cf24832541e7f4a2346a95275b
  bad b53bea5e2fcb30d3e00bd3409507a5659ce0fd8b
  current 1941b52820a544549596820a8ae006842b0e2c64
  good 3efc6fd51aeb8594398044c6c846ca59ae021203
  good 288867a866e9adb7a29880b66936c874b80f4651
  good 8e0c2264c8af790daf3585ada0669d93dee09c83
  good b5bd63375ab9a290419f2024b7f4ee9ea7ce90a8
  good ed2d2f24b11c368fa8aa0da9f4e1db580abade59
  good 58c80a7c8a4025a94cedaf7b4a4e3124e8909a96
  skip 9d7d07bc967ca98ad0600c24953fd289ad5fa991
  skip ce8f0998e922c179e80819d5066fbe46e2998784
  skip e7fa0811edb063f6319531f0d0a865882138e180
  skip a2e6ea4973e9196ddd3386493b0c214b41fd97d3

# bisect reverse test

  $ hg bisect -r
  $ hg bisect -b null
  $ hg bisect -g tip
  Testing changeset e7fa0811edb0 (32 changesets remaining, ~5 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset 03750880c6b5 (16 changesets remaining, ~4 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# skip

  $ hg bisect -s
  Testing changeset a3d5c6fdf0d3 (16 changesets remaining, ~4 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset db07c04beaca (7 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  Testing changeset b99c7b9c8e11 (3 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -b
  Testing changeset 5cd978ea5149 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  The first good revision is:
  commit:      5cd978ea5149
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     msg 1

  $ hg bisect -r
  $ hg bisect -g tip
  $ hg bisect -b tip
  abort: inconsistent state, 31:58c80a7c8a40 is good and bad
  [255]

  $ hg bisect -r
  $ hg bisect -g null
  $ hg bisect -bU tip
  Testing changeset e7fa0811edb0 (32 changesets remaining, ~5 tests)
  $ hg id
  5cd978ea5149

# Issue1228: hg bisect crashes when you skip the last rev in bisection
# Issue1182: hg bisect exception

  $ hg bisect -r
  $ hg bisect -b 4
  $ hg bisect -g 0
  Testing changeset db07c04beaca (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset 5cd978ea5149 (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset b53bea5e2fcb (4 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Due to skipped revisions, the first bad revision could be any of:
  commit:      5cd978ea5149
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     msg 1
  
  commit:      db07c04beaca
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     msg 2
  
  commit:      b53bea5e2fcb
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     msg 3
  
  commit:      9b2ba8336a65
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     msg 4

# reproduce non converging bisect, issue1182

  $ hg bisect -r
  $ hg bisect -g 0
  $ hg bisect -b 2
  Testing changeset 5cd978ea5149 (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Due to skipped revisions, the first bad revision could be any of:
  commit:      5cd978ea5149
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     msg 1
  
  commit:      db07c04beaca
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     msg 2

# test no action

  $ hg bisect -r
  $ hg bisect
  abort: cannot bisect (no known good revisions)
  [255]

# reproduce AssertionError, issue1445

  $ hg bisect -r
  $ hg bisect -b 6
  $ hg bisect -g 0
  Testing changeset b53bea5e2fcb (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset db07c04beaca (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset 9b2ba8336a65 (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset 5cd978ea5149 (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -s
  Testing changeset 7874a09ea728 (6 changesets remaining, ~2 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bisect -g
  The first bad revision is:
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6
  $ hg log -r 'bisect(good)'
  commit:      b99c7b9c8e11
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     msg 0
  
  commit:      7874a09ea728
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     msg 5
  $ hg log -r 'bisect(bad)'
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6
  $ hg log -r 'bisect(current)'
  commit:      7874a09ea728
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     msg 5
  $ hg log -r 'bisect(skip)'
  commit:      5cd978ea5149
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     msg 1
  
  commit:      db07c04beaca
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     msg 2
  
  commit:      b53bea5e2fcb
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     msg 3
  
  commit:      9b2ba8336a65
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     msg 4

# test legacy bisected() keyword

  $ hg log -r 'bisected(bad)'
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6

# test invalid command
# assuming that the shell returns 127 if command not found ...

  $ hg bisect -r
  $ hg bisect --command 'exit 127'
  abort: failed to execute exit 127
  [255]

# test bisecting command

  $ hg bisect -r
  $ hg up -qr tip
  $ 
hg bisect --command "hg debugshell -c \"sys.exit(1 if (repo['.'].rev() < 6) else 0)\""

  changeset 58c80a7c8a40: good
  abort: cannot bisect (no known bad revisions)
  [255]
  $ hg up -qr 0
  $ 
hg bisect --command "hg debugshell -c \"sys.exit(1 if (repo['.'].rev() < 6) else 0)\""

  changeset b99c7b9c8e11: bad
  changeset e7fa0811edb0: good
  changeset 03750880c6b5: good
  changeset b53bea5e2fcb: bad
  changeset 7874a09ea728: bad
  changeset a3d5c6fdf0d3: good
  The first good revision is:
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6

# test bisecting via a command without updating the working dir, and
# ensure that the bisect state file is updated before running a test
# command

  $ hg goto null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat > script.sh << 'EOF'
  > rev=$(hg log -r $HG_NODE --template '{rev}')
  > [ "$rev" -ge 6 ]
  > EOF
  $ chmod +x script.sh
  $ hg bisect -r
  $ hg bisect --good tip --noupdate
  $ hg bisect --bad 0 --noupdate
  Testing changeset e7fa0811edb0 (31 changesets remaining, ~4 tests)
  $ hg bisect --command 'sh script.sh and some params' --noupdate
  changeset e7fa0811edb0: good
  changeset 03750880c6b5: good
  changeset b53bea5e2fcb: bad
  changeset 7874a09ea728: bad
  changeset a3d5c6fdf0d3: good
  The first good revision is:
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6

# ensure that we still don't have a working dir

  $ hg parents

# test the same case, this time with updating

  $ hg bisect -r
  $ hg up -qr tip
  $ hg bisect --command 'sh script.sh and some params'
  changeset 58c80a7c8a40: good
  abort: cannot bisect (no known bad revisions)
  [255]
  $ hg up -qr 0
  $ hg bisect --command 'sh script.sh and some params'
  changeset b99c7b9c8e11: bad
  changeset e7fa0811edb0: good
  changeset 03750880c6b5: good
  changeset b53bea5e2fcb: bad
  changeset 7874a09ea728: bad
  changeset a3d5c6fdf0d3: good
  The first good revision is:
  commit:      a3d5c6fdf0d3
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     msg 6
  $ hg graft -q 15
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg bisect --reset
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Test the validation message when exclusive options are used:

  $ hg bisect -r
  $ hg bisect -b -c false
  abort: --bad and --command are incompatible
  [255]
  $ hg bisect -b -e
  abort: --bad and --extend are incompatible
  [255]
  $ hg bisect -b -g
  abort: --bad and --good are incompatible
  [255]
  $ hg bisect -b -r
  abort: --bad and --reset are incompatible
  [255]
  $ hg bisect -b -s
  abort: --bad and --skip are incompatible
  [255]
  $ hg bisect -c false -e
  abort: --command and --extend are incompatible
  [255]
  $ hg bisect -c false -g
  abort: --command and --good are incompatible
  [255]
  $ hg bisect -c false -r
  abort: --command and --reset are incompatible
  [255]
  $ hg bisect -c false -s
  abort: --command and --skip are incompatible
  [255]
  $ hg bisect -e -g
  abort: --extend and --good are incompatible
  [255]
  $ hg bisect -e -r
  abort: --extend and --reset are incompatible
  [255]
  $ hg bisect -e -s
  abort: --extend and --skip are incompatible
  [255]
  $ hg bisect -g -r
  abort: --good and --reset are incompatible
  [255]
  $ hg bisect -g -s
  abort: --good and --skip are incompatible
  [255]
  $ hg bisect -r -s
  abort: --reset and --skip are incompatible
  [255]
