# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"


# committing changes

sh % "echo" >> "a"
for i in range(32):
    sh % "echo a" >> "a"
    if i == 0:
        sh % "hg add" == r"adding a"
    sh % "hg ci -m 'msg {i}' -d '{i} 0'".format(i=i)

sh % "hg log" == r"""
    changeset:   31:58c80a7c8a40
    user:        test
    date:        Thu Jan 01 00:00:31 1970 +0000
    summary:     msg 31

    changeset:   30:ed2d2f24b11c
    user:        test
    date:        Thu Jan 01 00:00:30 1970 +0000
    summary:     msg 30

    changeset:   29:b5bd63375ab9
    user:        test
    date:        Thu Jan 01 00:00:29 1970 +0000
    summary:     msg 29

    changeset:   28:8e0c2264c8af
    user:        test
    date:        Thu Jan 01 00:00:28 1970 +0000
    summary:     msg 28

    changeset:   27:288867a866e9
    user:        test
    date:        Thu Jan 01 00:00:27 1970 +0000
    summary:     msg 27

    changeset:   26:3efc6fd51aeb
    user:        test
    date:        Thu Jan 01 00:00:26 1970 +0000
    summary:     msg 26

    changeset:   25:02a84173a97a
    user:        test
    date:        Thu Jan 01 00:00:25 1970 +0000
    summary:     msg 25

    changeset:   24:10e0acd3809e
    user:        test
    date:        Thu Jan 01 00:00:24 1970 +0000
    summary:     msg 24

    changeset:   23:5ec79163bff4
    user:        test
    date:        Thu Jan 01 00:00:23 1970 +0000
    summary:     msg 23

    changeset:   22:06c7993750ce
    user:        test
    date:        Thu Jan 01 00:00:22 1970 +0000
    summary:     msg 22

    changeset:   21:e5db6aa3fe2a
    user:        test
    date:        Thu Jan 01 00:00:21 1970 +0000
    summary:     msg 21

    changeset:   20:7128fb4fdbc9
    user:        test
    date:        Thu Jan 01 00:00:20 1970 +0000
    summary:     msg 20

    changeset:   19:52798545b482
    user:        test
    date:        Thu Jan 01 00:00:19 1970 +0000
    summary:     msg 19

    changeset:   18:86977a90077e
    user:        test
    date:        Thu Jan 01 00:00:18 1970 +0000
    summary:     msg 18

    changeset:   17:03515f4a9080
    user:        test
    date:        Thu Jan 01 00:00:17 1970 +0000
    summary:     msg 17

    changeset:   16:a2e6ea4973e9
    user:        test
    date:        Thu Jan 01 00:00:16 1970 +0000
    summary:     msg 16

    changeset:   15:e7fa0811edb0
    user:        test
    date:        Thu Jan 01 00:00:15 1970 +0000
    summary:     msg 15

    changeset:   14:ce8f0998e922
    user:        test
    date:        Thu Jan 01 00:00:14 1970 +0000
    summary:     msg 14

    changeset:   13:9d7d07bc967c
    user:        test
    date:        Thu Jan 01 00:00:13 1970 +0000
    summary:     msg 13

    changeset:   12:1941b52820a5
    user:        test
    date:        Thu Jan 01 00:00:12 1970 +0000
    summary:     msg 12

    changeset:   11:7b4cd9578619
    user:        test
    date:        Thu Jan 01 00:00:11 1970 +0000
    summary:     msg 11

    changeset:   10:7c5eff49a6b6
    user:        test
    date:        Thu Jan 01 00:00:10 1970 +0000
    summary:     msg 10

    changeset:   9:eb44510ef29a
    user:        test
    date:        Thu Jan 01 00:00:09 1970 +0000
    summary:     msg 9

    changeset:   8:453eb4dba229
    user:        test
    date:        Thu Jan 01 00:00:08 1970 +0000
    summary:     msg 8

    changeset:   7:03750880c6b5
    user:        test
    date:        Thu Jan 01 00:00:07 1970 +0000
    summary:     msg 7

    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6

    changeset:   5:7874a09ea728
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     msg 5

    changeset:   4:9b2ba8336a65
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     msg 4

    changeset:   3:b53bea5e2fcb
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     msg 3

    changeset:   2:db07c04beaca
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     msg 2

    changeset:   1:5cd978ea5149
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     msg 1

    changeset:   0:b99c7b9c8e11
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     msg 0"""

sh % "hg up -C" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# bisect test

sh % "hg bisect -r"
sh % "hg bisect -b"
sh % "hg status -v" == r"""
    # The repository is in an unfinished *bisect* state.

    # To mark the changeset good:    hg bisect --good
    # To mark the changeset bad:     hg bisect --bad
    # To abort:                      hg bisect --reset"""
sh % "hg status -v --config 'commands.status.skipstates=bisect'"
sh % "hg summary" == r"""
    parent: 31:58c80a7c8a40 
     msg 31
    commit: (clean)
    phases: 32 draft"""
sh % "hg bisect -g 1" == r"""
    Testing changeset a2e6ea4973e9 (30 changesets remaining, ~4 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset 5ec79163bff4 (15 changesets remaining, ~3 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# skip

sh % "hg bisect -s" == r"""
    Testing changeset 10e0acd3809e (15 changesets remaining, ~3 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset 288867a866e9 (7 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset b5bd63375ab9 (4 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -b" == r"""
    Testing changeset 8e0c2264c8af (2 changesets remaining, ~1 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    The first bad revision is:
    changeset:   29:b5bd63375ab9
    user:        test
    date:        Thu Jan 01 00:00:29 1970 +0000
    summary:     msg 29"""

# mark revsets instead of single revs

sh % "hg bisect -r"
sh % "hg bisect -b '0::3'"
sh % "hg bisect -s '13::16'"
sh % "hg bisect -g '26::tip'" == r"""
    Testing changeset 1941b52820a5 (23 changesets remaining, ~4 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cat .hg/bisect.state" == r"""
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
    skip a2e6ea4973e9196ddd3386493b0c214b41fd97d3"""

# bisect reverse test

sh % "hg bisect -r"
sh % "hg bisect -b null"
sh % "hg bisect -g tip" == r"""
    Testing changeset e7fa0811edb0 (32 changesets remaining, ~5 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset 03750880c6b5 (16 changesets remaining, ~4 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# skip

sh % "hg bisect -s" == r"""
    Testing changeset a3d5c6fdf0d3 (16 changesets remaining, ~4 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset db07c04beaca (7 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    Testing changeset b99c7b9c8e11 (3 changesets remaining, ~1 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -b" == r"""
    Testing changeset 5cd978ea5149 (2 changesets remaining, ~1 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    The first good revision is:
    changeset:   1:5cd978ea5149
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     msg 1"""

sh % "hg bisect -r"
sh % "hg bisect -g tip"
sh % "hg bisect -b tip" == r"""
    abort: inconsistent state, 31:58c80a7c8a40 is good and bad
    [255]"""

sh % "hg bisect -r"
sh % "hg bisect -g null"
sh % "hg bisect -bU tip" == "Testing changeset e7fa0811edb0 (32 changesets remaining, ~5 tests)"
sh % "hg id" == "5cd978ea5149"


# Issue1228: hg bisect crashes when you skip the last rev in bisection
# Issue1182: hg bisect exception

sh % "hg bisect -r"
sh % "hg bisect -b 4"
sh % "hg bisect -g 0" == r"""
    Testing changeset db07c04beaca (4 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset 5cd978ea5149 (4 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset b53bea5e2fcb (4 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Due to skipped revisions, the first bad revision could be any of:
    changeset:   1:5cd978ea5149
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     msg 1

    changeset:   2:db07c04beaca
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     msg 2

    changeset:   3:b53bea5e2fcb
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     msg 3

    changeset:   4:9b2ba8336a65
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     msg 4"""


# reproduce non converging bisect, issue1182

sh % "hg bisect -r"
sh % "hg bisect -g 0"
sh % "hg bisect -b 2" == r"""
    Testing changeset 5cd978ea5149 (2 changesets remaining, ~1 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Due to skipped revisions, the first bad revision could be any of:
    changeset:   1:5cd978ea5149
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     msg 1

    changeset:   2:db07c04beaca
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     msg 2"""


# test no action

sh % "hg bisect -r"
sh % "hg bisect" == r"""
    abort: cannot bisect (no known good revisions)
    [255]"""


# reproduce AssertionError, issue1445

sh % "hg bisect -r"
sh % "hg bisect -b 6"
sh % "hg bisect -g 0" == r"""
    Testing changeset b53bea5e2fcb (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset db07c04beaca (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset 9b2ba8336a65 (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset 5cd978ea5149 (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -s" == r"""
    Testing changeset 7874a09ea728 (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect -g" == r"""
    The first bad revision is:
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""
sh % "hg log -r 'bisect(good)'" == r"""
    changeset:   0:b99c7b9c8e11
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     msg 0

    changeset:   5:7874a09ea728
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     msg 5"""
sh % "hg log -r 'bisect(bad)'" == r"""
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""
sh % "hg log -r 'bisect(current)'" == r"""
    changeset:   5:7874a09ea728
    user:        test
    date:        Thu Jan 01 00:00:05 1970 +0000
    summary:     msg 5"""
sh % "hg log -r 'bisect(skip)'" == r"""
    changeset:   1:5cd978ea5149
    user:        test
    date:        Thu Jan 01 00:00:01 1970 +0000
    summary:     msg 1

    changeset:   2:db07c04beaca
    user:        test
    date:        Thu Jan 01 00:00:02 1970 +0000
    summary:     msg 2

    changeset:   3:b53bea5e2fcb
    user:        test
    date:        Thu Jan 01 00:00:03 1970 +0000
    summary:     msg 3

    changeset:   4:9b2ba8336a65
    user:        test
    date:        Thu Jan 01 00:00:04 1970 +0000
    summary:     msg 4"""

# test legacy bisected() keyword

sh % "hg log -r 'bisected(bad)'" == r"""
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""

# test invalid command
# assuming that the shell returns 127 if command not found ...

sh % "hg bisect -r"
sh % "hg bisect --command 'exit 127'" == r"""
    abort: failed to execute exit 127
    [255]"""


# test bisecting command

sh % "cat" << r"""
from __future__ import absolute_import
import sys
from edenscm.mercurial import hg, ui as uimod
repo = hg.repository(uimod.ui.load(), '.')
if repo['.'].rev() < 6:
    sys.exit(1)
""" > "script.py"
sh % "hg bisect -r"
sh % "hg up -qr tip"
sh % "hg bisect --command 'hg debugpython -- script.py and some parameters'" == r"""
    changeset 31:58c80a7c8a40: good
    abort: cannot bisect (no known bad revisions)
    [255]"""
sh % "hg up -qr 0"
sh % "hg bisect --command 'hg debugpython -- script.py and some parameters'" == r"""
    changeset 0:b99c7b9c8e11: bad
    changeset 15:e7fa0811edb0: good
    changeset 7:03750880c6b5: good
    changeset 3:b53bea5e2fcb: bad
    changeset 5:7874a09ea728: bad
    changeset 6:a3d5c6fdf0d3: good
    The first good revision is:
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""


# test bisecting via a command without updating the working dir, and
# ensure that the bisect state file is updated before running a test
# command

sh % "hg update null" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "cat" << r"""
#!/bin/sh
test -n "$HG_NODE" || (echo HG_NODE missing; exit 127)
current="`hg log -r \"bisect(current)\" --template {node}`"
test "$current" = "$HG_NODE" || (echo current is bad: $current; exit 127)
rev="`hg log -r $HG_NODE --template {rev}`"
test "$rev" -ge 6
""" > "script.sh"
sh % "chmod +x script.sh"
sh % "hg bisect -r"
sh % "hg bisect --good tip --noupdate"
sh % "hg bisect --bad 0 --noupdate" == "Testing changeset e7fa0811edb0 (31 changesets remaining, ~4 tests)"
sh % "hg bisect --command 'sh script.sh and some params' --noupdate" == r"""
    changeset 15:e7fa0811edb0: good
    changeset 7:03750880c6b5: good
    changeset 3:b53bea5e2fcb: bad
    changeset 5:7874a09ea728: bad
    changeset 6:a3d5c6fdf0d3: good
    The first good revision is:
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""

# ensure that we still don't have a working dir

sh % "hg parents"


# test the same case, this time with updating

sh % "cat" << r"""
#!/bin/sh
test -n "$HG_NODE" || (echo HG_NODE missing; exit 127)
current="`hg log -r \"bisect(current)\" --template {node}`"
test "$current" = "$HG_NODE" || (echo current is bad: $current; exit 127)
rev="`hg log -r . --template {rev}`"
test "$rev" -ge 6
""" > "script.sh"
sh % "chmod +x script.sh"
sh % "hg bisect -r"
sh % "hg up -qr tip"
sh % "hg bisect --command 'sh script.sh and some params'" == r"""
    changeset 31:58c80a7c8a40: good
    abort: cannot bisect (no known bad revisions)
    [255]"""
sh % "hg up -qr 0"
sh % "hg bisect --command 'sh script.sh and some params'" == r"""
    changeset 0:b99c7b9c8e11: bad
    changeset 15:e7fa0811edb0: good
    changeset 7:03750880c6b5: good
    changeset 3:b53bea5e2fcb: bad
    changeset 5:7874a09ea728: bad
    changeset 6:a3d5c6fdf0d3: good
    The first good revision is:
    changeset:   6:a3d5c6fdf0d3
    user:        test
    date:        Thu Jan 01 00:00:06 1970 +0000
    summary:     msg 6"""
sh % "hg graft -q 15" == r"""
    warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
    abort: unresolved conflicts, can't continue
    (use 'hg resolve' and 'hg graft --continue')
    [255]"""
sh % "hg bisect --reset"
sh % "hg up -C ." == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

# Check that bisect does not break on obsolete changesets
# =========================================================

sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> "$HGRCPATH"

# tip is obsolete
# ---------------------

cln = (sh % "hg id --debug -i -r tip").output
sh % "hg debugobsolete {}".format(cln) == "obsoleted 1 changesets"
sh % "hg bisect --reset"
sh % "hg bisect --good 15"
sh % "hg bisect --bad 30" == r"""
    Testing changeset 06c7993750ce (15 changesets remaining, ~3 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect --command true" == r"""
    changeset 22:06c7993750ce: good
    changeset 26:3efc6fd51aeb: good
    changeset 28:8e0c2264c8af: good
    changeset 29:b5bd63375ab9: good
    The first bad revision is:
    changeset:   30:ed2d2f24b11c
    user:        test
    date:        Thu Jan 01 00:00:30 1970 +0000
    summary:     msg 30"""

# Changeset in the bad:good range is obsolete
# ---------------------------------------------

sh % "hg up 30" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo a" >> "a"
sh % "hg ci -m 'msg 32' -d '32 0'"
sh % "hg bisect --reset"
sh % "hg bisect --good ."
sh % "hg bisect --bad 25" == r"""
    Testing changeset 8e0c2264c8af (6 changesets remaining, ~2 tests)
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "hg bisect --command true" == r"""
    changeset 28:8e0c2264c8af: good
    changeset 26:3efc6fd51aeb: good
    The first good revision is:
    changeset:   26:3efc6fd51aeb
    user:        test
    date:        Thu Jan 01 00:00:26 1970 +0000
    summary:     msg 26"""
# Test the validation message when exclusive options are used:

sh % "hg bisect -r"
sh % "hg bisect -b -c false" == r"""
    abort: --bad and --command are incompatible
    [255]"""
sh % "hg bisect -b -e" == r"""
    abort: --bad and --extend are incompatible
    [255]"""
sh % "hg bisect -b -g" == r"""
    abort: --bad and --good are incompatible
    [255]"""
sh % "hg bisect -b -r" == r"""
    abort: --bad and --reset are incompatible
    [255]"""
sh % "hg bisect -b -s" == r"""
    abort: --bad and --skip are incompatible
    [255]"""
sh % "hg bisect -c false -e" == r"""
    abort: --command and --extend are incompatible
    [255]"""
sh % "hg bisect -c false -g" == r"""
    abort: --command and --good are incompatible
    [255]"""
sh % "hg bisect -c false -r" == r"""
    abort: --command and --reset are incompatible
    [255]"""
sh % "hg bisect -c false -s" == r"""
    abort: --command and --skip are incompatible
    [255]"""
sh % "hg bisect -e -g" == r"""
    abort: --extend and --good are incompatible
    [255]"""
sh % "hg bisect -e -r" == r"""
    abort: --extend and --reset are incompatible
    [255]"""
sh % "hg bisect -e -s" == r"""
    abort: --extend and --skip are incompatible
    [255]"""
sh % "hg bisect -g -r" == r"""
    abort: --good and --reset are incompatible
    [255]"""
sh % "hg bisect -g -s" == r"""
    abort: --good and --skip are incompatible
    [255]"""
sh % "hg bisect -r -s" == r"""
    abort: --reset and --skip are incompatible
    [255]"""
