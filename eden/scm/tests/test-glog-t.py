# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


# @  (34) head
# |
# | o  (33) head
# | |
# o |    (32) expand
# |\ \
# | o \    (31) expand
# | |\ \
# | | o \    (30) expand
# | | |\ \
# | | | o |  (29) regular commit
# | | | | |
# | | o | |    (28) merge zero known
# | | |\ \ \
# o | | | | |  (27) collapse
# |/ / / / /
# | | o---+  (26) merge one known; far right
# | | | | |
# +---o | |  (25) merge one known; far left
# | | | | |
# | | o | |  (24) merge one known; immediate right
# | | |\| |
# | | o | |  (23) merge one known; immediate left
# | |/| | |
# +---o---+  (22) merge two known; one far left, one far right
# | |  / /
# o | | |    (21) expand
# |\ \ \ \
# | o---+-+  (20) merge two known; two far right
# |  / / /
# o | | |    (19) expand
# |\ \ \ \
# +---+---o  (18) merge two known; two far left
# | | | |
# | o | |    (17) expand
# | |\ \ \
# | | o---+  (16) merge two known; one immediate right, one near right
# | | |/ /
# o | | |    (15) expand
# |\ \ \ \
# | o-----+  (14) merge two known; one immediate right, one far right
# | |/ / /
# o | | |    (13) expand
# |\ \ \ \
# +---o | |  (12) merge two known; one immediate right, one far left
# | | |/ /
# | o | |    (11) expand
# | |\ \ \
# | | o---+  (10) merge two known; one immediate left, one near right
# | |/ / /
# o | | |    (9) expand
# |\ \ \ \
# | o-----+  (8) merge two known; one immediate left, one far right
# |/ / / /
# o | | |    (7) expand
# |\ \ \ \
# +---o | |  (6) merge two known; one immediate left, one far left
# | |/ / /
# | o | |    (5) expand
# | |\ \ \
# | | o | |  (4) merge two known; one immediate left, one immediate right
# | |/|/ /
# | o / /  (3) collapse
# |/ / /
# o / /  (2) collapse
# |/ /
# o /  (1) collapse
# |/
# o  (0) root


def commit(rev, msg, *args):
    if args:
        sh.hg("debugsetparents", *args)
    open("a", "wb").write("%s\n" % rev)
    sh.hg("commit", "-Aqd", "%s 0" % rev, "-m", "(%s) %s" % (rev, msg))


shlib.__dict__["commit"] = commit


sh % "cat" << r"""
from __future__ import absolute_import
from edenscm.mercurial import (
  cmdutil,
  commands,
  extensions,
  revsetlang,
)

def uisetup(ui):
    def printrevset(orig, ui, repo, *pats, **opts):
        if opts.get('print_revset'):
            expr = cmdutil.getgraphlogrevs(repo, pats, opts)[1]
            if expr:
                tree = revsetlang.parse(expr)
            else:
                tree = []
            ui.write('%r\n' % (opts.get('rev', []),))
            ui.write(revsetlang.prettyformat(tree) + '\n')
            return 0
        return orig(ui, repo, *pats, **opts)
    entry = extensions.wrapcommand(commands.table, 'log', printrevset)
    entry[1].append(('', 'print-revset', False,
                     'print generated revset and exit (DEPRECATED)'))
""" > "printrevset.py"

sh % "echo '[extensions]'" >> "$HGRCPATH"
sh % "echo 'printrevset=$TESTTMP/printrevset.py'" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

# Empty repo:

sh % "hg log -G"


# Building DAG:

sh % "commit 0 root"
sh % "commit 1 collapse 0"
sh % "commit 2 collapse 1"
sh % "commit 3 collapse 2"
sh % "commit 4 'merge two known; one immediate left, one immediate right' 1 3"
sh % "commit 5 expand 3 4"
sh % "commit 6 'merge two known; one immediate left, one far left' 2 5"
sh % "commit 7 expand 2 5"
sh % "commit 8 'merge two known; one immediate left, one far right' 0 7"
sh % "commit 9 expand 7 8"
sh % "commit 10 'merge two known; one immediate left, one near right' 0 6"
sh % "commit 11 expand 6 10"
sh % "commit 12 'merge two known; one immediate right, one far left' 1 9"
sh % "commit 13 expand 9 11"
sh % "commit 14 'merge two known; one immediate right, one far right' 0 12"
sh % "commit 15 expand 13 14"
sh % "commit 16 'merge two known; one immediate right, one near right' 0 1"
sh % "commit 17 expand 12 16"
sh % "commit 18 'merge two known; two far left' 1 15"
sh % "commit 19 expand 15 17"
sh % "commit 20 'merge two known; two far right' 0 18"
sh % "commit 21 expand 19 20"
sh % "commit 22 'merge two known; one far left, one far right' 18 21"
sh % "commit 23 'merge one known; immediate left' 1 22"
sh % "commit 24 'merge one known; immediate right' 0 23"
sh % "commit 25 'merge one known; far left' 21 24"
sh % "commit 26 'merge one known; far right' 18 25"
sh % "commit 27 collapse 21"
sh % "commit 28 'merge zero known' 1 26"
sh % "commit 29 'regular commit' 0"
sh % "commit 30 expand 28 29"
sh % "commit 31 expand 21 30"
sh % "commit 32 expand 27 31"
sh % "commit 33 head 18"
sh % "commit 34 head 32"


sh % "hg log -G -q" == r"""
    @  34:fea3ac5810e0
    |
    | o  33:68608f5145f9
    | |
    o |    32:d06dffa21a31
    |\ \
    | o \    31:621d83e11f67
    | |\ \
    | | o \    30:6e11cd4b648f
    | | |\ \
    | | | o |  29:cd9bb2be7593
    | | | | |
    | | o | |    28:44ecd0b9ae99
    | | |\ \ \
    o | | | | |  27:886ed638191b
    |/ / / / /
    | | o---+  26:7f25b6c2f0b9
    | | | | |
    +---o | |  25:91da8ed57247
    | | | | |
    | | o | |  24:a9c19a3d96b7
    | | |\| |
    | | o | |  23:a01cddf0766d
    | |/| | |
    +---o---+  22:e0d9cccacb5d
    | |  / /
    o | | |    21:d42a756af44d
    |\ \ \ \
    | o---+-+  20:d30ed6450e32
    |  / / /
    o | | |    19:31ddc2c1573b
    |\ \ \ \
    +---+---o  18:1aa84d96232a
    | | | |
    | o | |    17:44765d7c06e0
    | |\ \ \
    | | o---+  16:3677d192927d
    | | |/ /
    o | | |    15:1dda3f72782d
    |\ \ \ \
    | o-----+  14:8eac370358ef
    | |/ / /
    o | | |    13:22d8966a97e3
    |\ \ \ \
    +---o | |  12:86b91144a6e9
    | | |/ /
    | o | |    11:832d76e6bdf2
    | |\ \ \
    | | o---+  10:74c64d036d72
    | |/ / /
    o | | |    9:7010c0af0a35
    |\ \ \ \
    | o-----+  8:7a0b11f71937
    |/ / / /
    o | | |    7:b632bb1b1224
    |\ \ \ \
    +---o | |  6:b105a072e251
    | |/ / /
    | o | |    5:4409d547b708
    | |\ \ \
    | | o | |  4:26a8bac39d9f
    | |/|/ /
    | o / /  3:27eef8ed80b4
    |/ / /
    o / /  2:3d9a33b8d1e1
    |/ /
    o /  1:6db2ef61d156
    |/
    o  0:e6eb3150255d"""

sh % "hg log -G" == r"""
    @  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    | |  parent:      18:1aa84d96232a
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:33 1970 +0000
    | |  summary:     (33) head
    | |
    o |    changeset:   32:d06dffa21a31
    |\ \   parent:      27:886ed638191b
    | | |  parent:      31:621d83e11f67
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:32 1970 +0000
    | | |  summary:     (32) expand
    | | |
    | o |    changeset:   31:621d83e11f67
    | |\ \   parent:      21:d42a756af44d
    | | | |  parent:      30:6e11cd4b648f
    | | | |  user:        test
    | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
    | | | |  summary:     (31) expand
    | | | |
    | | o |    changeset:   30:6e11cd4b648f
    | | |\ \   parent:      28:44ecd0b9ae99
    | | | | |  parent:      29:cd9bb2be7593
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
    | | | | |  summary:     (30) expand
    | | | | |
    | | | o |  changeset:   29:cd9bb2be7593
    | | | | |  parent:      0:e6eb3150255d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
    | | | | |  summary:     (29) regular commit
    | | | | |
    | | o | |    changeset:   28:44ecd0b9ae99
    | | |\ \ \   parent:      1:6db2ef61d156
    | | | | | |  parent:      26:7f25b6c2f0b9
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
    | | | | | |  summary:     (28) merge zero known
    | | | | | |
    o | | | | |  changeset:   27:886ed638191b
    |/ / / / /   parent:      21:d42a756af44d
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
    | | | | |    summary:     (27) collapse
    | | | | |
    | | o---+  changeset:   26:7f25b6c2f0b9
    | | | | |  parent:      18:1aa84d96232a
    | | | | |  parent:      25:91da8ed57247
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
    | | | | |  summary:     (26) merge one known; far right
    | | | | |
    +---o | |  changeset:   25:91da8ed57247
    | | | | |  parent:      21:d42a756af44d
    | | | | |  parent:      24:a9c19a3d96b7
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
    | | | | |  summary:     (25) merge one known; far left
    | | | | |
    | | o | |  changeset:   24:a9c19a3d96b7
    | | |\| |  parent:      0:e6eb3150255d
    | | | | |  parent:      23:a01cddf0766d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
    | | | | |  summary:     (24) merge one known; immediate right
    | | | | |
    | | o | |  changeset:   23:a01cddf0766d
    | |/| | |  parent:      1:6db2ef61d156
    | | | | |  parent:      22:e0d9cccacb5d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
    | | | | |  summary:     (23) merge one known; immediate left
    | | | | |
    +---o---+  changeset:   22:e0d9cccacb5d
    | |   | |  parent:      18:1aa84d96232a
    | |  / /   parent:      21:d42a756af44d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
    | | | |    summary:     (22) merge two known; one far left, one far right
    | | | |
    o | | |    changeset:   21:d42a756af44d
    |\ \ \ \   parent:      19:31ddc2c1573b
    | | | | |  parent:      20:d30ed6450e32
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | | | |  summary:     (21) expand
    | | | | |
    | o---+-+  changeset:   20:d30ed6450e32
    |   | | |  parent:      0:e6eb3150255d
    |  / / /   parent:      18:1aa84d96232a
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | | | |    summary:     (20) merge two known; two far right
    | | | |
    o | | |    changeset:   19:31ddc2c1573b
    |\ \ \ \   parent:      15:1dda3f72782d
    | | | | |  parent:      17:44765d7c06e0
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | | | |  summary:     (19) expand
    | | | | |
    +---+---o  changeset:   18:1aa84d96232a
    | | | |    parent:      1:6db2ef61d156
    | | | |    parent:      15:1dda3f72782d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
    | | | |    summary:     (18) merge two known; two far left
    | | | |
    | o | |    changeset:   17:44765d7c06e0
    | |\ \ \   parent:      12:86b91144a6e9
    | | | | |  parent:      16:3677d192927d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | | | |  summary:     (17) expand
    | | | | |
    | | o---+  changeset:   16:3677d192927d
    | | | | |  parent:      0:e6eb3150255d
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
    | | | |    summary:     (16) merge two known; one immediate right, one near right
    | | | |
    o | | |    changeset:   15:1dda3f72782d
    |\ \ \ \   parent:      13:22d8966a97e3
    | | | | |  parent:      14:8eac370358ef
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | | | |  summary:     (15) expand
    | | | | |
    | o-----+  changeset:   14:8eac370358ef
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      12:86b91144a6e9
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
    | | | |    summary:     (14) merge two known; one immediate right, one far right
    | | | |
    o | | |    changeset:   13:22d8966a97e3
    |\ \ \ \   parent:      9:7010c0af0a35
    | | | | |  parent:      11:832d76e6bdf2
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | | | |  summary:     (13) expand
    | | | | |
    +---o | |  changeset:   12:86b91144a6e9
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    parent:      9:7010c0af0a35
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | | | |    summary:     (12) merge two known; one immediate right, one far left
    | | | |
    | o | |    changeset:   11:832d76e6bdf2
    | |\ \ \   parent:      6:b105a072e251
    | | | | |  parent:      10:74c64d036d72
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | | | |  summary:     (11) expand
    | | | | |
    | | o---+  changeset:   10:74c64d036d72
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      6:b105a072e251
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | | | |    summary:     (10) merge two known; one immediate left, one near right
    | | | |
    o | | |    changeset:   9:7010c0af0a35
    |\ \ \ \   parent:      7:b632bb1b1224
    | | | | |  parent:      8:7a0b11f71937
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | | | |  summary:     (9) expand
    | | | | |
    | o-----+  changeset:   8:7a0b11f71937
    | | | | |  parent:      0:e6eb3150255d
    |/ / / /   parent:      7:b632bb1b1224
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
    | | | |    summary:     (8) merge two known; one immediate left, one far right
    | | | |
    o | | |    changeset:   7:b632bb1b1224
    |\ \ \ \   parent:      2:3d9a33b8d1e1
    | | | | |  parent:      5:4409d547b708
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
    | | | | |  summary:     (7) expand
    | | | | |
    +---o | |  changeset:   6:b105a072e251
    | |/ / /   parent:      2:3d9a33b8d1e1
    | | | |    parent:      5:4409d547b708
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
    | | | |    summary:     (6) merge two known; one immediate left, one far left
    | | | |
    | o | |    changeset:   5:4409d547b708
    | |\ \ \   parent:      3:27eef8ed80b4
    | | | | |  parent:      4:26a8bac39d9f
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
    | | | | |  summary:     (5) expand
    | | | | |
    | | o | |  changeset:   4:26a8bac39d9f
    | |/|/ /   parent:      1:6db2ef61d156
    | | | |    parent:      3:27eef8ed80b4
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
    | | | |    summary:     (4) merge two known; one immediate left, one immediate right
    | | | |
    | o | |  changeset:   3:27eef8ed80b4
    |/ / /   user:        test
    | | |    date:        Thu Jan 01 00:00:03 1970 +0000
    | | |    summary:     (3) collapse
    | | |
    o | |  changeset:   2:3d9a33b8d1e1
    |/ /   user:        test
    | |    date:        Thu Jan 01 00:00:02 1970 +0000
    | |    summary:     (2) collapse
    | |
    o |  changeset:   1:6db2ef61d156
    |/   user:        test
    |    date:        Thu Jan 01 00:00:01 1970 +0000
    |    summary:     (1) collapse
    |
    o  changeset:   0:e6eb3150255d
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     (0) root"""

# File glog:
sh % "hg log -G a" == r"""
    @  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    | |  parent:      18:1aa84d96232a
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:33 1970 +0000
    | |  summary:     (33) head
    | |
    o |    changeset:   32:d06dffa21a31
    |\ \   parent:      27:886ed638191b
    | | |  parent:      31:621d83e11f67
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:32 1970 +0000
    | | |  summary:     (32) expand
    | | |
    | o |    changeset:   31:621d83e11f67
    | |\ \   parent:      21:d42a756af44d
    | | | |  parent:      30:6e11cd4b648f
    | | | |  user:        test
    | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
    | | | |  summary:     (31) expand
    | | | |
    | | o |    changeset:   30:6e11cd4b648f
    | | |\ \   parent:      28:44ecd0b9ae99
    | | | | |  parent:      29:cd9bb2be7593
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
    | | | | |  summary:     (30) expand
    | | | | |
    | | | o |  changeset:   29:cd9bb2be7593
    | | | | |  parent:      0:e6eb3150255d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
    | | | | |  summary:     (29) regular commit
    | | | | |
    | | o | |    changeset:   28:44ecd0b9ae99
    | | |\ \ \   parent:      1:6db2ef61d156
    | | | | | |  parent:      26:7f25b6c2f0b9
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
    | | | | | |  summary:     (28) merge zero known
    | | | | | |
    o | | | | |  changeset:   27:886ed638191b
    |/ / / / /   parent:      21:d42a756af44d
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
    | | | | |    summary:     (27) collapse
    | | | | |
    | | o---+  changeset:   26:7f25b6c2f0b9
    | | | | |  parent:      18:1aa84d96232a
    | | | | |  parent:      25:91da8ed57247
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
    | | | | |  summary:     (26) merge one known; far right
    | | | | |
    +---o | |  changeset:   25:91da8ed57247
    | | | | |  parent:      21:d42a756af44d
    | | | | |  parent:      24:a9c19a3d96b7
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
    | | | | |  summary:     (25) merge one known; far left
    | | | | |
    | | o | |  changeset:   24:a9c19a3d96b7
    | | |\| |  parent:      0:e6eb3150255d
    | | | | |  parent:      23:a01cddf0766d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
    | | | | |  summary:     (24) merge one known; immediate right
    | | | | |
    | | o | |  changeset:   23:a01cddf0766d
    | |/| | |  parent:      1:6db2ef61d156
    | | | | |  parent:      22:e0d9cccacb5d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
    | | | | |  summary:     (23) merge one known; immediate left
    | | | | |
    +---o---+  changeset:   22:e0d9cccacb5d
    | |   | |  parent:      18:1aa84d96232a
    | |  / /   parent:      21:d42a756af44d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
    | | | |    summary:     (22) merge two known; one far left, one far right
    | | | |
    o | | |    changeset:   21:d42a756af44d
    |\ \ \ \   parent:      19:31ddc2c1573b
    | | | | |  parent:      20:d30ed6450e32
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | | | |  summary:     (21) expand
    | | | | |
    | o---+-+  changeset:   20:d30ed6450e32
    |   | | |  parent:      0:e6eb3150255d
    |  / / /   parent:      18:1aa84d96232a
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | | | |    summary:     (20) merge two known; two far right
    | | | |
    o | | |    changeset:   19:31ddc2c1573b
    |\ \ \ \   parent:      15:1dda3f72782d
    | | | | |  parent:      17:44765d7c06e0
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | | | |  summary:     (19) expand
    | | | | |
    +---+---o  changeset:   18:1aa84d96232a
    | | | |    parent:      1:6db2ef61d156
    | | | |    parent:      15:1dda3f72782d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
    | | | |    summary:     (18) merge two known; two far left
    | | | |
    | o | |    changeset:   17:44765d7c06e0
    | |\ \ \   parent:      12:86b91144a6e9
    | | | | |  parent:      16:3677d192927d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | | | |  summary:     (17) expand
    | | | | |
    | | o---+  changeset:   16:3677d192927d
    | | | | |  parent:      0:e6eb3150255d
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
    | | | |    summary:     (16) merge two known; one immediate right, one near right
    | | | |
    o | | |    changeset:   15:1dda3f72782d
    |\ \ \ \   parent:      13:22d8966a97e3
    | | | | |  parent:      14:8eac370358ef
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | | | |  summary:     (15) expand
    | | | | |
    | o-----+  changeset:   14:8eac370358ef
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      12:86b91144a6e9
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
    | | | |    summary:     (14) merge two known; one immediate right, one far right
    | | | |
    o | | |    changeset:   13:22d8966a97e3
    |\ \ \ \   parent:      9:7010c0af0a35
    | | | | |  parent:      11:832d76e6bdf2
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | | | |  summary:     (13) expand
    | | | | |
    +---o | |  changeset:   12:86b91144a6e9
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    parent:      9:7010c0af0a35
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | | | |    summary:     (12) merge two known; one immediate right, one far left
    | | | |
    | o | |    changeset:   11:832d76e6bdf2
    | |\ \ \   parent:      6:b105a072e251
    | | | | |  parent:      10:74c64d036d72
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | | | |  summary:     (11) expand
    | | | | |
    | | o---+  changeset:   10:74c64d036d72
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      6:b105a072e251
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | | | |    summary:     (10) merge two known; one immediate left, one near right
    | | | |
    o | | |    changeset:   9:7010c0af0a35
    |\ \ \ \   parent:      7:b632bb1b1224
    | | | | |  parent:      8:7a0b11f71937
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | | | |  summary:     (9) expand
    | | | | |
    | o-----+  changeset:   8:7a0b11f71937
    | | | | |  parent:      0:e6eb3150255d
    |/ / / /   parent:      7:b632bb1b1224
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
    | | | |    summary:     (8) merge two known; one immediate left, one far right
    | | | |
    o | | |    changeset:   7:b632bb1b1224
    |\ \ \ \   parent:      2:3d9a33b8d1e1
    | | | | |  parent:      5:4409d547b708
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
    | | | | |  summary:     (7) expand
    | | | | |
    +---o | |  changeset:   6:b105a072e251
    | |/ / /   parent:      2:3d9a33b8d1e1
    | | | |    parent:      5:4409d547b708
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
    | | | |    summary:     (6) merge two known; one immediate left, one far left
    | | | |
    | o | |    changeset:   5:4409d547b708
    | |\ \ \   parent:      3:27eef8ed80b4
    | | | | |  parent:      4:26a8bac39d9f
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
    | | | | |  summary:     (5) expand
    | | | | |
    | | o | |  changeset:   4:26a8bac39d9f
    | |/|/ /   parent:      1:6db2ef61d156
    | | | |    parent:      3:27eef8ed80b4
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
    | | | |    summary:     (4) merge two known; one immediate left, one immediate right
    | | | |
    | o | |  changeset:   3:27eef8ed80b4
    |/ / /   user:        test
    | | |    date:        Thu Jan 01 00:00:03 1970 +0000
    | | |    summary:     (3) collapse
    | | |
    o | |  changeset:   2:3d9a33b8d1e1
    |/ /   user:        test
    | |    date:        Thu Jan 01 00:00:02 1970 +0000
    | |    summary:     (2) collapse
    | |
    o |  changeset:   1:6db2ef61d156
    |/   user:        test
    |    date:        Thu Jan 01 00:00:01 1970 +0000
    |    summary:     (1) collapse
    |
    o  changeset:   0:e6eb3150255d
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     (0) root"""

# File glog per revset:

sh % "hg log -G -r 'file(\"a\")'" == r"""
    @  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    | |  parent:      18:1aa84d96232a
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:33 1970 +0000
    | |  summary:     (33) head
    | |
    o |    changeset:   32:d06dffa21a31
    |\ \   parent:      27:886ed638191b
    | | |  parent:      31:621d83e11f67
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:32 1970 +0000
    | | |  summary:     (32) expand
    | | |
    | o |    changeset:   31:621d83e11f67
    | |\ \   parent:      21:d42a756af44d
    | | | |  parent:      30:6e11cd4b648f
    | | | |  user:        test
    | | | |  date:        Thu Jan 01 00:00:31 1970 +0000
    | | | |  summary:     (31) expand
    | | | |
    | | o |    changeset:   30:6e11cd4b648f
    | | |\ \   parent:      28:44ecd0b9ae99
    | | | | |  parent:      29:cd9bb2be7593
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:30 1970 +0000
    | | | | |  summary:     (30) expand
    | | | | |
    | | | o |  changeset:   29:cd9bb2be7593
    | | | | |  parent:      0:e6eb3150255d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:29 1970 +0000
    | | | | |  summary:     (29) regular commit
    | | | | |
    | | o | |    changeset:   28:44ecd0b9ae99
    | | |\ \ \   parent:      1:6db2ef61d156
    | | | | | |  parent:      26:7f25b6c2f0b9
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
    | | | | | |  summary:     (28) merge zero known
    | | | | | |
    o | | | | |  changeset:   27:886ed638191b
    |/ / / / /   parent:      21:d42a756af44d
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:27 1970 +0000
    | | | | |    summary:     (27) collapse
    | | | | |
    | | o---+  changeset:   26:7f25b6c2f0b9
    | | | | |  parent:      18:1aa84d96232a
    | | | | |  parent:      25:91da8ed57247
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
    | | | | |  summary:     (26) merge one known; far right
    | | | | |
    +---o | |  changeset:   25:91da8ed57247
    | | | | |  parent:      21:d42a756af44d
    | | | | |  parent:      24:a9c19a3d96b7
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
    | | | | |  summary:     (25) merge one known; far left
    | | | | |
    | | o | |  changeset:   24:a9c19a3d96b7
    | | |\| |  parent:      0:e6eb3150255d
    | | | | |  parent:      23:a01cddf0766d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
    | | | | |  summary:     (24) merge one known; immediate right
    | | | | |
    | | o | |  changeset:   23:a01cddf0766d
    | |/| | |  parent:      1:6db2ef61d156
    | | | | |  parent:      22:e0d9cccacb5d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
    | | | | |  summary:     (23) merge one known; immediate left
    | | | | |
    +---o---+  changeset:   22:e0d9cccacb5d
    | |   | |  parent:      18:1aa84d96232a
    | |  / /   parent:      21:d42a756af44d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
    | | | |    summary:     (22) merge two known; one far left, one far right
    | | | |
    o | | |    changeset:   21:d42a756af44d
    |\ \ \ \   parent:      19:31ddc2c1573b
    | | | | |  parent:      20:d30ed6450e32
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | | | |  summary:     (21) expand
    | | | | |
    | o---+-+  changeset:   20:d30ed6450e32
    |   | | |  parent:      0:e6eb3150255d
    |  / / /   parent:      18:1aa84d96232a
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | | | |    summary:     (20) merge two known; two far right
    | | | |
    o | | |    changeset:   19:31ddc2c1573b
    |\ \ \ \   parent:      15:1dda3f72782d
    | | | | |  parent:      17:44765d7c06e0
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | | | |  summary:     (19) expand
    | | | | |
    +---+---o  changeset:   18:1aa84d96232a
    | | | |    parent:      1:6db2ef61d156
    | | | |    parent:      15:1dda3f72782d
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
    | | | |    summary:     (18) merge two known; two far left
    | | | |
    | o | |    changeset:   17:44765d7c06e0
    | |\ \ \   parent:      12:86b91144a6e9
    | | | | |  parent:      16:3677d192927d
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | | | |  summary:     (17) expand
    | | | | |
    | | o---+  changeset:   16:3677d192927d
    | | | | |  parent:      0:e6eb3150255d
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
    | | | |    summary:     (16) merge two known; one immediate right, one near right
    | | | |
    o | | |    changeset:   15:1dda3f72782d
    |\ \ \ \   parent:      13:22d8966a97e3
    | | | | |  parent:      14:8eac370358ef
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | | | |  summary:     (15) expand
    | | | | |
    | o-----+  changeset:   14:8eac370358ef
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      12:86b91144a6e9
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
    | | | |    summary:     (14) merge two known; one immediate right, one far right
    | | | |
    o | | |    changeset:   13:22d8966a97e3
    |\ \ \ \   parent:      9:7010c0af0a35
    | | | | |  parent:      11:832d76e6bdf2
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | | | |  summary:     (13) expand
    | | | | |
    +---o | |  changeset:   12:86b91144a6e9
    | | |/ /   parent:      1:6db2ef61d156
    | | | |    parent:      9:7010c0af0a35
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | | | |    summary:     (12) merge two known; one immediate right, one far left
    | | | |
    | o | |    changeset:   11:832d76e6bdf2
    | |\ \ \   parent:      6:b105a072e251
    | | | | |  parent:      10:74c64d036d72
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | | | |  summary:     (11) expand
    | | | | |
    | | o---+  changeset:   10:74c64d036d72
    | | | | |  parent:      0:e6eb3150255d
    | |/ / /   parent:      6:b105a072e251
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | | | |    summary:     (10) merge two known; one immediate left, one near right
    | | | |
    o | | |    changeset:   9:7010c0af0a35
    |\ \ \ \   parent:      7:b632bb1b1224
    | | | | |  parent:      8:7a0b11f71937
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | | | |  summary:     (9) expand
    | | | | |
    | o-----+  changeset:   8:7a0b11f71937
    | | | | |  parent:      0:e6eb3150255d
    |/ / / /   parent:      7:b632bb1b1224
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
    | | | |    summary:     (8) merge two known; one immediate left, one far right
    | | | |
    o | | |    changeset:   7:b632bb1b1224
    |\ \ \ \   parent:      2:3d9a33b8d1e1
    | | | | |  parent:      5:4409d547b708
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
    | | | | |  summary:     (7) expand
    | | | | |
    +---o | |  changeset:   6:b105a072e251
    | |/ / /   parent:      2:3d9a33b8d1e1
    | | | |    parent:      5:4409d547b708
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
    | | | |    summary:     (6) merge two known; one immediate left, one far left
    | | | |
    | o | |    changeset:   5:4409d547b708
    | |\ \ \   parent:      3:27eef8ed80b4
    | | | | |  parent:      4:26a8bac39d9f
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
    | | | | |  summary:     (5) expand
    | | | | |
    | | o | |  changeset:   4:26a8bac39d9f
    | |/|/ /   parent:      1:6db2ef61d156
    | | | |    parent:      3:27eef8ed80b4
    | | | |    user:        test
    | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
    | | | |    summary:     (4) merge two known; one immediate left, one immediate right
    | | | |
    | o | |  changeset:   3:27eef8ed80b4
    |/ / /   user:        test
    | | |    date:        Thu Jan 01 00:00:03 1970 +0000
    | | |    summary:     (3) collapse
    | | |
    o | |  changeset:   2:3d9a33b8d1e1
    |/ /   user:        test
    | |    date:        Thu Jan 01 00:00:02 1970 +0000
    | |    summary:     (2) collapse
    | |
    o |  changeset:   1:6db2ef61d156
    |/   user:        test
    |    date:        Thu Jan 01 00:00:01 1970 +0000
    |    summary:     (1) collapse
    |
    o  changeset:   0:e6eb3150255d
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     (0) root"""


# File glog per revset (only merges):

sh % "hg log -G -r 'file(\"a\")' -m" == r"""
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    | :  parent:      31:621d83e11f67
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:32 1970 +0000
    | :  summary:     (32) expand
    | :
    o :  changeset:   31:621d83e11f67
    |\:  parent:      21:d42a756af44d
    | :  parent:      30:6e11cd4b648f
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:31 1970 +0000
    | :  summary:     (31) expand
    | :
    o :    changeset:   30:6e11cd4b648f
    |\ \   parent:      28:44ecd0b9ae99
    | ~ :  parent:      29:cd9bb2be7593
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:30 1970 +0000
    |   :  summary:     (30) expand
    |  /
    o :    changeset:   28:44ecd0b9ae99
    |\ \   parent:      1:6db2ef61d156
    | ~ :  parent:      26:7f25b6c2f0b9
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:28 1970 +0000
    |   :  summary:     (28) merge zero known
    |  /
    o :    changeset:   26:7f25b6c2f0b9
    |\ \   parent:      18:1aa84d96232a
    | | :  parent:      25:91da8ed57247
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:26 1970 +0000
    | | :  summary:     (26) merge one known; far right
    | | :
    | o :  changeset:   25:91da8ed57247
    | |\:  parent:      21:d42a756af44d
    | | :  parent:      24:a9c19a3d96b7
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:25 1970 +0000
    | | :  summary:     (25) merge one known; far left
    | | :
    | o :    changeset:   24:a9c19a3d96b7
    | |\ \   parent:      0:e6eb3150255d
    | | ~ :  parent:      23:a01cddf0766d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:24 1970 +0000
    | |   :  summary:     (24) merge one known; immediate right
    | |  /
    | o :    changeset:   23:a01cddf0766d
    | |\ \   parent:      1:6db2ef61d156
    | | ~ :  parent:      22:e0d9cccacb5d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:23 1970 +0000
    | |   :  summary:     (23) merge one known; immediate left
    | |  /
    | o :  changeset:   22:e0d9cccacb5d
    |/:/   parent:      18:1aa84d96232a
    | :    parent:      21:d42a756af44d
    | :    user:        test
    | :    date:        Thu Jan 01 00:00:22 1970 +0000
    | :    summary:     (22) merge two known; one far left, one far right
    | :
    | o    changeset:   21:d42a756af44d
    | |\   parent:      19:31ddc2c1573b
    | | |  parent:      20:d30ed6450e32
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | |  summary:     (21) expand
    | | |
    +---o  changeset:   20:d30ed6450e32
    | | |  parent:      0:e6eb3150255d
    | | ~  parent:      18:1aa84d96232a
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | |    summary:     (20) merge two known; two far right
    | |
    | o    changeset:   19:31ddc2c1573b
    | |\   parent:      15:1dda3f72782d
    | | |  parent:      17:44765d7c06e0
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | |  summary:     (19) expand
    | | |
    o | |  changeset:   18:1aa84d96232a
    |\| |  parent:      1:6db2ef61d156
    ~ | |  parent:      15:1dda3f72782d
      | |  user:        test
      | |  date:        Thu Jan 01 00:00:18 1970 +0000
      | |  summary:     (18) merge two known; two far left
     / /
    | o    changeset:   17:44765d7c06e0
    | |\   parent:      12:86b91144a6e9
    | | |  parent:      16:3677d192927d
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | |  summary:     (17) expand
    | | |
    | | o    changeset:   16:3677d192927d
    | | |\   parent:      0:e6eb3150255d
    | | ~ ~  parent:      1:6db2ef61d156
    | |      user:        test
    | |      date:        Thu Jan 01 00:00:16 1970 +0000
    | |      summary:     (16) merge two known; one immediate right, one near right
    | |
    o |    changeset:   15:1dda3f72782d
    |\ \   parent:      13:22d8966a97e3
    | | |  parent:      14:8eac370358ef
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | |  summary:     (15) expand
    | | |
    | o |  changeset:   14:8eac370358ef
    | |\|  parent:      0:e6eb3150255d
    | ~ |  parent:      12:86b91144a6e9
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:14 1970 +0000
    |   |  summary:     (14) merge two known; one immediate right, one far right
    |  /
    o |    changeset:   13:22d8966a97e3
    |\ \   parent:      9:7010c0af0a35
    | | |  parent:      11:832d76e6bdf2
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | |  summary:     (13) expand
    | | |
    +---o  changeset:   12:86b91144a6e9
    | | |  parent:      1:6db2ef61d156
    | | ~  parent:      9:7010c0af0a35
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | |    summary:     (12) merge two known; one immediate right, one far left
    | |
    | o    changeset:   11:832d76e6bdf2
    | |\   parent:      6:b105a072e251
    | | |  parent:      10:74c64d036d72
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | |  summary:     (11) expand
    | | |
    | | o  changeset:   10:74c64d036d72
    | |/|  parent:      0:e6eb3150255d
    | | ~  parent:      6:b105a072e251
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | |    summary:     (10) merge two known; one immediate left, one near right
    | |
    o |    changeset:   9:7010c0af0a35
    |\ \   parent:      7:b632bb1b1224
    | | |  parent:      8:7a0b11f71937
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | |  summary:     (9) expand
    | | |
    | o |  changeset:   8:7a0b11f71937
    |/| |  parent:      0:e6eb3150255d
    | ~ |  parent:      7:b632bb1b1224
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:08 1970 +0000
    |   |  summary:     (8) merge two known; one immediate left, one far right
    |  /
    o |    changeset:   7:b632bb1b1224
    |\ \   parent:      2:3d9a33b8d1e1
    | ~ |  parent:      5:4409d547b708
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:07 1970 +0000
    |   |  summary:     (7) expand
    |  /
    | o  changeset:   6:b105a072e251
    |/|  parent:      2:3d9a33b8d1e1
    | ~  parent:      5:4409d547b708
    |    user:        test
    |    date:        Thu Jan 01 00:00:06 1970 +0000
    |    summary:     (6) merge two known; one immediate left, one far left
    |
    o    changeset:   5:4409d547b708
    |\   parent:      3:27eef8ed80b4
    | ~  parent:      4:26a8bac39d9f
    |    user:        test
    |    date:        Thu Jan 01 00:00:05 1970 +0000
    |    summary:     (5) expand
    |
    o    changeset:   4:26a8bac39d9f
    |\   parent:      1:6db2ef61d156
    ~ ~  parent:      3:27eef8ed80b4
         user:        test
         date:        Thu Jan 01 00:00:04 1970 +0000
         summary:     (4) merge two known; one immediate left, one immediate right"""


# Empty revision range - display nothing:
sh % "hg log -G -r 1..0"

sh % "cd .."

if feature.check(["no-outer-repo"]):

    # From outer space:
    sh % "hg log -G -l1 repo" == r"""
        @  changeset:   34:fea3ac5810e0
        ~  parent:      32:d06dffa21a31
           user:        test
           date:        Thu Jan 01 00:00:34 1970 +0000
           summary:     (34) head"""
    sh % "hg log -G -l1 repo/a" == r"""
        @  changeset:   34:fea3ac5810e0
        ~  parent:      32:d06dffa21a31
           user:        test
           date:        Thu Jan 01 00:00:34 1970 +0000
           summary:     (34) head"""
    sh % "hg log -G -l1 repo/missing"


# File log with revs != cset revs:
sh % "hg init flog"
sh % "cd flog"
sh % "echo one" > "one"
sh % "hg add one"
sh % "hg commit -mone"
sh % "echo two" > "two"
sh % "hg add two"
sh % "hg commit -mtwo"
sh % "echo more" > "two"
sh % "hg commit -mmore"
sh % "hg log -G two" == r"""
    @  changeset:   2:12c28321755b
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     more
    |
    o  changeset:   1:5ac72c0599bf
    |  user:        test
    ~  date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     two"""

# Issue1896: File log with explicit style
sh % "hg log -G '--style=default' one" == r"""
    o  changeset:   0:3d578b4a1f53
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     one"""
# Issue2395: glog --style header and footer
sh % "hg log -G '--style=xml' one" == r"""
    <?xml version="1.0"?>
    <log>
    o  <logentry revision="0" node="3d578b4a1f537d5fcf7301bfa9c0b97adfaa6fb1">
       <author email="test">test</author>
       <date>1970-01-01T00:00:00+00:00</date>
       <msg xml:space="preserve">one</msg>
       </logentry>
    </log>"""

sh % "cd .."

# Incoming and outgoing:

sh % "hg clone -U -r31 repo repo2" == r"""
    adding changesets
    adding manifests
    adding file changes
    added 31 changesets with 31 changes to 1 files
    new changesets e6eb3150255d:621d83e11f67"""
sh % "cd repo2"

sh % "hg incoming --graph ../repo" == r"""
    comparing with ../repo
    searching for changes
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    |    parent:      18:1aa84d96232a
    |    user:        test
    |    date:        Thu Jan 01 00:00:33 1970 +0000
    |    summary:     (33) head
    |
    o  changeset:   32:d06dffa21a31
    |  parent:      27:886ed638191b
    |  parent:      31:621d83e11f67
    |  user:        test
    |  date:        Thu Jan 01 00:00:32 1970 +0000
    |  summary:     (32) expand
    |
    o  changeset:   27:886ed638191b
       parent:      21:d42a756af44d
       user:        test
       date:        Thu Jan 01 00:00:27 1970 +0000
       summary:     (27) collapse"""
sh % "cd .."

sh % "hg -R repo outgoing --graph repo2" == r"""
    comparing with repo2
    searching for changes
    @  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    |    parent:      18:1aa84d96232a
    |    user:        test
    |    date:        Thu Jan 01 00:00:33 1970 +0000
    |    summary:     (33) head
    |
    o  changeset:   32:d06dffa21a31
    |  parent:      27:886ed638191b
    |  parent:      31:621d83e11f67
    |  user:        test
    |  date:        Thu Jan 01 00:00:32 1970 +0000
    |  summary:     (32) expand
    |
    o  changeset:   27:886ed638191b
       parent:      21:d42a756af44d
       user:        test
       date:        Thu Jan 01 00:00:27 1970 +0000
       summary:     (27) collapse"""

# File + limit with revs != cset revs:
sh % "cd repo"
sh % "touch b"
sh % "hg ci -Aqm0"
sh % "hg log -G -l2 a" == r"""
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    ~  user:        test
       date:        Thu Jan 01 00:00:34 1970 +0000
       summary:     (34) head

    o  changeset:   33:68608f5145f9
    |  parent:      18:1aa84d96232a
    ~  user:        test
       date:        Thu Jan 01 00:00:33 1970 +0000
       summary:     (33) head"""

# File + limit + -ra:b, (b - a) < limit:
sh % "hg log -G -l3000 '-r32:tip' a" == r"""
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    | |  parent:      18:1aa84d96232a
    | ~  user:        test
    |    date:        Thu Jan 01 00:00:33 1970 +0000
    |    summary:     (33) head
    |
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    ~ ~  parent:      31:621d83e11f67
         user:        test
         date:        Thu Jan 01 00:00:32 1970 +0000
         summary:     (32) expand"""

# Point out a common and an uncommon unshown parent

sh % "hg log -G -r 'rev(8) or rev(9)'" == r"""
    o    changeset:   9:7010c0af0a35
    |\   parent:      7:b632bb1b1224
    | ~  parent:      8:7a0b11f71937
    |    user:        test
    |    date:        Thu Jan 01 00:00:09 1970 +0000
    |    summary:     (9) expand
    |
    o    changeset:   8:7a0b11f71937
    |\   parent:      0:e6eb3150255d
    ~ ~  parent:      7:b632bb1b1224
         user:        test
         date:        Thu Jan 01 00:00:08 1970 +0000
         summary:     (8) merge two known; one immediate left, one far right"""

# File + limit + -ra:b, b < tip:

sh % "hg log -G -l1 '-r32:34' a" == r"""
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    ~  user:        test
       date:        Thu Jan 01 00:00:34 1970 +0000
       summary:     (34) head"""

# file(File) + limit + -ra:b, b < tip:

sh % "hg log -G -l1 '-r32:34' -r 'file(\"a\")'" == r"""
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    ~  user:        test
       date:        Thu Jan 01 00:00:34 1970 +0000
       summary:     (34) head"""

# limit(file(File) and a::b), b < tip:

sh % "hg log -G -r 'limit(file(\"a\") and 32::34, 1)'" == r"""
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    ~ ~  parent:      31:621d83e11f67
         user:        test
         date:        Thu Jan 01 00:00:32 1970 +0000
         summary:     (32) expand"""

# File + limit + -ra:b, b < tip:

sh % "hg log -G -r 'limit(file(\"a\") and 34::32, 1)'"

# File + limit + -ra:b, b < tip, (b - a) < limit:

sh % "hg log -G -l10 '-r33:34' a" == r"""
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    ~  user:        test
       date:        Thu Jan 01 00:00:34 1970 +0000
       summary:     (34) head

    o  changeset:   33:68608f5145f9
    |  parent:      18:1aa84d96232a
    ~  user:        test
       date:        Thu Jan 01 00:00:33 1970 +0000
       summary:     (33) head"""

# Do not crash or produce strange graphs if history is buggy

sh % "commit 36 'buggy merge: identical parents' 35 35"
sh % "hg log -G -l5" == r"""
    @  changeset:   36:95fa8febd08a
    |  parent:      35:9159c3644c5e
    |  parent:      35:9159c3644c5e
    |  user:        test
    |  date:        Thu Jan 01 00:00:36 1970 +0000
    |  summary:     (36) buggy merge: identical parents
    |
    o  changeset:   35:9159c3644c5e
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     0
    |
    o  changeset:   34:fea3ac5810e0
    |  parent:      32:d06dffa21a31
    |  user:        test
    |  date:        Thu Jan 01 00:00:34 1970 +0000
    |  summary:     (34) head
    |
    | o  changeset:   33:68608f5145f9
    | |  parent:      18:1aa84d96232a
    | ~  user:        test
    |    date:        Thu Jan 01 00:00:33 1970 +0000
    |    summary:     (33) head
    |
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    ~ ~  parent:      31:621d83e11f67
         user:        test
         date:        Thu Jan 01 00:00:32 1970 +0000
         summary:     (32) expand"""

# Test log -G options

# glog always reorders nodes which explains the difference with log

sh % "hg log -G --print-revset -r 27 -r 25 -r 21 -r 34 -r 32 -r 31" == r"""
    ['27', '25', '21', '34', '32', '31']
    []"""
sh % "hg log -G --print-revset -u test -u not-a-user" == r"""
    []
    (group
      (group
        (or
          (list
            (func
              (symbol 'user')
              (string 'test'))
            (func
              (symbol 'user')
              (string 'not-a-user'))))))"""
sh % "hg log -G --print-revset -k expand -k merge" == r"""
    []
    (group
      (group
        (or
          (list
            (func
              (symbol 'keyword')
              (string 'expand'))
            (func
              (symbol 'keyword')
              (string 'merge'))))))"""
sh % "hg log -G --print-revset --only-merges" == r"""
    []
    (group
      (func
        (symbol 'merge')
        None))"""
sh % "hg log -G --print-revset --no-merges" == r"""
    []
    (group
      (not
        (func
          (symbol 'merge')
          None)))"""
sh % "hg log -G --print-revset --date '2 0 to 4 0'" == r"""
    []
    (group
      (func
        (symbol 'date')
        (string '2 0 to 4 0')))"""
sh % "hg log -G -d 'brace ) in a date'" == r"""
    hg: parse error: invalid date: 'brace ) in a date'
    [255]"""
sh % "hg log -G --print-revset --prune 31 --prune 32" == r"""
    []
    (group
      (group
        (and
          (not
            (group
              (or
                (list
                  (string '31')
                  (func
                    (symbol 'ancestors')
                    (string '31'))))))
          (not
            (group
              (or
                (list
                  (string '32')
                  (func
                    (symbol 'ancestors')
                    (string '32')))))))))"""

# Dedicated repo for --follow and paths filtering. The g is crafted to
# have 2 filelog topological heads in a linear changeset graph.

sh % "cd .."
sh % "hg init follow"
sh % "cd follow"
sh % "hg log -G --print-revset --follow" == r"""
    []
    []"""
sh % "hg log -G --print-revset -rnull" == r"""
    ['null']
    []"""
sh % "echo a" > "a"
sh % "echo aa" > "aa"
sh % "echo f" > "f"
sh % "hg ci -Am 'add a' a aa f"
sh % "hg cp a b"
sh % "hg cp f g"
sh % "hg ci -m 'copy a b'"
sh % "mkdir dir"
sh % "hg mv b dir"
sh % "echo g" >> "g"
sh % "echo f" >> "f"
sh % "hg ci -m 'mv b dir/b'"
sh % "hg mv a b"
sh % "hg cp -f f g"
sh % "echo a" > "d"
sh % "hg add d"
sh % "hg ci -m 'mv a b; add d'"
sh % "hg mv dir/b e"
sh % "hg ci -m 'mv dir/b e'"
sh % "hg log -G --template '({rev}) {desc|firstline}\\n'" == r"""
    @  (4) mv dir/b e
    |
    o  (3) mv a b; add d
    |
    o  (2) mv b dir/b
    |
    o  (1) copy a b
    |
    o  (0) add a"""

sh % "hg log -G --print-revset a" == r"""
    []
    (group
      (group
        (func
          (symbol 'filelog')
          (string 'a'))))"""
sh % "hg log -G --print-revset a b" == r"""
    []
    (group
      (group
        (or
          (list
            (func
              (symbol 'filelog')
              (string 'a'))
            (func
              (symbol 'filelog')
              (string 'b'))))))"""

# Test falling back to slow path for non-existing files

sh % "hg log -G --print-revset a c" == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:a')
          (string 'p:c'))))"""

# Test multiple --include/--exclude/paths

sh % "hg log -G --print-revset --include a --include e --exclude b --exclude e a e" == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:a')
          (string 'p:e')
          (string 'i:a')
          (string 'i:e')
          (string 'x:b')
          (string 'x:e'))))"""

# Test glob expansion of pats

# Unsupported by t.py: glob pattern is not expanded - not a real shell.
if feature.check("false"):
    sh % "hg log -G --print-revset 'a*'" == r"""
        []
        (group
          (group
            (func
              (symbol 'filelog')
              (string 'aa'))))"""

# Test --follow on a non-existent directory

sh % "hg log -G --print-revset -f dir" == r"""
    abort: cannot follow file not in parent revision: "dir"
    [255]"""

# Test --follow on a directory

sh % "hg up -q '.^'"
sh % "hg log -G --print-revset -f dir" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'dir'))))"""
sh % "hg up -q tip"

# Test --follow on file not in parent revision

sh % "hg log -G --print-revset -f a" == r"""
    abort: cannot follow file not in parent revision: "a"
    [255]"""

# Test --follow and patterns

sh % "hg log -G --print-revset -f 'glob:*'" == r"""
    []
    (group
      (and
        (func
          (symbol 'ancestors')
          (symbol '.'))
        (func
          (symbol '_matchfiles')
          (list
            (string 'r:')
            (string 'd:relpath')
            (string 'p:glob:*')))))"""

# Test --follow on a single rename

sh % "hg up -q 2"
sh % "hg log -G --print-revset -f a" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'a'))))"""

# Test --follow and multiple renames

sh % "hg up -q tip"
sh % "hg log -G --print-revset -f e" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'e'))))"""

# Test --follow and multiple filelog heads

sh % "hg up -q 2"
sh % "hg log -G --print-revset -f g" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'g'))))"""
sh % "hg up -q tip"
sh % "hg log -G --print-revset -f g" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'g'))))"""

# Test --follow and multiple files

sh % "hg log -G --print-revset -f g e" == r"""
    []
    (group
      (group
        (or
          (list
            (func
              (symbol 'follow')
              (string 'g'))
            (func
              (symbol 'follow')
              (string 'e'))))))"""

# Test --follow null parent

sh % "hg up -q null"
sh % "hg log -G --print-revset -f" == r"""
    []
    []"""

# Test --follow-first

sh % "hg up -q 3"
sh % "echo ee" > "e"
sh % "hg ci -Am 'add another e' e"
sh % "hg merge --tool 'internal:other' 4" == r"""
    0 files updated, 1 files merged, 1 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "echo merge" > "e"
sh % "hg ci -m 'merge 5 and 4'"
sh % "hg log -G --print-revset --follow-first" == r"""
    []
    (group
      (func
        (symbol '_firstancestors')
        (func
          (symbol 'rev')
          (symbol '6'))))"""

# Cannot compare with log --follow-first FILE as it never worked

sh % "hg log -G --print-revset --follow-first e" == r"""
    []
    (group
      (group
        (func
          (symbol '_followfirst')
          (string 'e'))))"""
sh % "hg log -G --follow-first e --template '{rev} {desc|firstline}\\n'" == r"""
    @    6 merge 5 and 4
    |\
    | ~
    o  5 add another e
    |
    ~"""

# Test --copies

sh % "hg log -G --copies --template '{rev} {desc|firstline}   copies: {file_copies_switch}\\n'" == r"""
    @    6 merge 5 and 4   copies:
    |\
    | o  5 add another e   copies:
    | |
    o |  4 mv dir/b e   copies: e (dir/b)
    |/
    o  3 mv a b; add d   copies: b (a)g (f)
    |
    o  2 mv b dir/b   copies: dir/b (b)
    |
    o  1 copy a b   copies: b (a)g (f)
    |
    o  0 add a   copies:"""
# Test "set:..." and parent revision

sh % "hg up -q 4"
sh % "hg log -G --print-revset 'set:copied()'" == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:set:copied()'))))"""
sh % "hg log -G --print-revset --include 'set:copied()'" == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'i:set:copied()'))))"""
sh % "hg log -G --print-revset -r 'sort(file('\\''set:copied()'\\''), -rev)'" == r"""
    ["sort(file('set:copied()'), -rev)"]
    []"""

# Test --removed

sh % "hg log -G --print-revset --removed" == r"""
    []
    []"""
sh % "hg log -G --print-revset --removed a" == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:a'))))"""
sh % "hg log -G --print-revset --removed --follow a" == r"""
    []
    (group
      (and
        (func
          (symbol 'ancestors')
          (symbol '.'))
        (func
          (symbol '_matchfiles')
          (list
            (string 'r:')
            (string 'd:relpath')
            (string 'p:a')))))"""

# Test --patch and --stat with --follow and --follow-first

sh % "hg up -q 3"
sh % "hg log -G --git --patch b" == r"""
    o  changeset:   1:216d4c92cf98
    |  user:        test
    ~  date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     copy a b

       diff --git a/a b/b
       copy from a
       copy to b"""

sh % "hg log -G --git --stat b" == r"""
    o  changeset:   1:216d4c92cf98
    |  user:        test
    ~  date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     copy a b

        b |  0
        1 files changed, 0 insertions(+), 0 deletions(-)"""

sh % "hg log -G --git --patch --follow b" == r"""
    o  changeset:   1:216d4c92cf98
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     copy a b
    |
    |  diff --git a/a b/b
    |  copy from a
    |  copy to b
    |
    o  changeset:   0:f8035bb17114
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     add a

       diff --git a/a b/a
       new file mode 100644
       --- /dev/null
       +++ b/a
       @@ -0,0 +1,1 @@
       +a"""

sh % "hg log -G --git --stat --follow b" == r"""
    o  changeset:   1:216d4c92cf98
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     copy a b
    |
    |   b |  0
    |   1 files changed, 0 insertions(+), 0 deletions(-)
    |
    o  changeset:   0:f8035bb17114
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     add a

        a |  1 +
        1 files changed, 1 insertions(+), 0 deletions(-)"""

sh % "hg up -q 6"
sh % "hg log -G --git --patch --follow-first e" == r"""
    @    changeset:   6:fc281d8ff18d
    |\   parent:      5:99b31f1c2782
    | ~  parent:      4:17d952250a9d
    |    user:        test
    |    date:        Thu Jan 01 00:00:00 1970 +0000
    |    summary:     merge 5 and 4
    |
    |    diff --git a/e b/e
    |    --- a/e
    |    +++ b/e
    |    @@ -1,1 +1,1 @@
    |    -ee
    |    +merge
    |
    o  changeset:   5:99b31f1c2782
    |  parent:      3:5918b8d165d1
    ~  user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     add another e

       diff --git a/e b/e
       new file mode 100644
       --- /dev/null
       +++ b/e
       @@ -0,0 +1,1 @@
       +ee"""

# Test old-style --rev

sh % "hg tag foo-bar"
sh % "hg log -G --print-revset -r foo-bar" == r"""
    ['foo-bar']
    []"""

# Test --follow and forward --rev

sh % "hg up -q 6"
sh % "echo g" > "g"
sh % "hg ci -Am 'add g' g"
sh % "hg up -q 2"
sh % "hg log -G --template '{rev} {desc|firstline}\\n'" == r"""
    o  8 add g
    |
    | o  7 Added tag foo-bar for changeset fc281d8ff18d
    |/
    o    6 merge 5 and 4
    |\
    | o  5 add another e
    | |
    o |  4 mv dir/b e
    |/
    o  3 mv a b; add d
    |
    @  2 mv b dir/b
    |
    o  1 copy a b
    |
    o  0 add a"""
sh % "hg archive -r 7 archive"
sh % "rm -r archive"

# changessincelatesttag with no prior tag
sh % "hg archive -r 4 archive"

sh % "hg export 'all()'" == r"""
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID f8035bb17114da16215af3436ec5222428ace8ee
    # Parent  0000000000000000000000000000000000000000
    add a

    diff -r 000000000000 -r f8035bb17114 a
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r 000000000000 -r f8035bb17114 aa
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/aa	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +aa
    diff -r 000000000000 -r f8035bb17114 f
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/f	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +f
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 216d4c92cf98ff2b4641d508b76b529f3d424c92
    # Parent  f8035bb17114da16215af3436ec5222428ace8ee
    copy a b

    diff -r f8035bb17114 -r 216d4c92cf98 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r f8035bb17114 -r 216d4c92cf98 g
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/g	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +f
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID bb573313a9e8349099b6ea2b2fb1fc7f424446f3
    # Parent  216d4c92cf98ff2b4641d508b76b529f3d424c92
    mv b dir/b

    diff -r 216d4c92cf98 -r bb573313a9e8 b
    --- a/b	Thu Jan 01 00:00:00 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r 216d4c92cf98 -r bb573313a9e8 dir/b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/dir/b	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r 216d4c92cf98 -r bb573313a9e8 f
    --- a/f	Thu Jan 01 00:00:00 1970 +0000
    +++ b/f	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,2 @@
     f
    +f
    diff -r 216d4c92cf98 -r bb573313a9e8 g
    --- a/g	Thu Jan 01 00:00:00 1970 +0000
    +++ b/g	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,2 @@
     f
    +g
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 5918b8d165d1364e78a66d02e66caa0133c5d1ed
    # Parent  bb573313a9e8349099b6ea2b2fb1fc7f424446f3
    mv a b; add d

    diff -r bb573313a9e8 -r 5918b8d165d1 a
    --- a/a	Thu Jan 01 00:00:00 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r bb573313a9e8 -r 5918b8d165d1 b
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/b	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r bb573313a9e8 -r 5918b8d165d1 d
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/d	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    diff -r bb573313a9e8 -r 5918b8d165d1 g
    --- a/g	Thu Jan 01 00:00:00 1970 +0000
    +++ b/g	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,2 +1,2 @@
     f
    -g
    +f
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 17d952250a9d03cc3dc77b199ab60e959b9b0260
    # Parent  5918b8d165d1364e78a66d02e66caa0133c5d1ed
    mv dir/b e

    diff -r 5918b8d165d1 -r 17d952250a9d dir/b
    --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r 5918b8d165d1 -r 17d952250a9d e
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/e	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +a
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 99b31f1c2782e2deb1723cef08930f70fc84b37b
    # Parent  5918b8d165d1364e78a66d02e66caa0133c5d1ed
    add another e

    diff -r 5918b8d165d1 -r 99b31f1c2782 e
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/e	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +ee
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID fc281d8ff18d999ad6497b3d27390bcd695dcc73
    # Parent  99b31f1c2782e2deb1723cef08930f70fc84b37b
    # Parent  17d952250a9d03cc3dc77b199ab60e959b9b0260
    merge 5 and 4

    diff -r 99b31f1c2782 -r fc281d8ff18d dir/b
    --- a/dir/b	Thu Jan 01 00:00:00 1970 +0000
    +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +0,0 @@
    -a
    diff -r 99b31f1c2782 -r fc281d8ff18d e
    --- a/e	Thu Jan 01 00:00:00 1970 +0000
    +++ b/e	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -ee
    +merge
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 02dbb8e276b8ab7abfd07cab50c901647e75c2dd
    # Parent  fc281d8ff18d999ad6497b3d27390bcd695dcc73
    Added tag foo-bar for changeset fc281d8ff18d

    diff -r fc281d8ff18d -r 02dbb8e276b8 .hgtags
    --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
    +++ b/.hgtags	Thu Jan 01 00:00:00 1970 +0000
    @@ -0,0 +1,1 @@
    +fc281d8ff18d999ad6497b3d27390bcd695dcc73 foo-bar
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 24c2e826ddebf80f9dcd60b856bdb8e6715c5449
    # Parent  fc281d8ff18d999ad6497b3d27390bcd695dcc73
    add g

    diff -r fc281d8ff18d -r 24c2e826ddeb g
    --- a/g	Thu Jan 01 00:00:00 1970 +0000
    +++ b/g	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,2 +1,1 @@
    -f
    -f
    +g"""
sh % "hg log -G --print-revset --follow -r6 -r8 -r5 -r7 -r4" == r"""
    ['6', '8', '5', '7', '4']
    (group
      (func
        (symbol 'descendants')
        (func
          (symbol 'rev')
          (symbol '6'))))"""

# Test --follow-first and forward --rev

sh % "hg log -G --print-revset --follow-first -r6 -r8 -r5 -r7 -r4" == r"""
    ['6', '8', '5', '7', '4']
    (group
      (func
        (symbol '_firstdescendants')
        (func
          (symbol 'rev')
          (symbol '6'))))"""

# Test --follow and backward --rev

sh % "hg log -G --print-revset --follow -r6 -r5 -r7 -r8 -r4" == r"""
    ['6', '5', '7', '8', '4']
    (group
      (func
        (symbol 'ancestors')
        (func
          (symbol 'rev')
          (symbol '6'))))"""

# Test --follow-first and backward --rev

sh % "hg log -G --print-revset --follow-first -r6 -r5 -r7 -r8 -r4" == r"""
    ['6', '5', '7', '8', '4']
    (group
      (func
        (symbol '_firstancestors')
        (func
          (symbol 'rev')
          (symbol '6'))))"""

# Test subdir

sh % "hg up -q 3"
sh % "cd dir"
sh % "hg log -G --print-revset ." == r"""
    []
    (group
      (func
        (symbol '_matchfiles')
        (list
          (string 'r:')
          (string 'd:relpath')
          (string 'p:.'))))"""
sh % "hg log -G --print-revset ../b" == r"""
    []
    (group
      (group
        (func
          (symbol 'filelog')
          (string '../b'))))"""
sh % "hg log -G --print-revset -f ../b" == r"""
    []
    (group
      (group
        (func
          (symbol 'follow')
          (string 'b'))))"""
sh % "cd .."

# Test --hidden
#  (enable obsolete)

sh % "cat" << r"""
[experimental]
evolution.createmarkers=True
""" >> "$HGRCPATH"

node = sh.hg("log", "-r8", "-T{node}")
sh % ("hg debugobsolete '%s'" % node) == "obsoleted 1 changesets"
sh % "hg log -G --print-revset" == r"""
    []
    []"""
sh % "hg log -G --print-revset --hidden" == r"""
    []
    []"""
sh % "hg log -G --template '{rev} {desc}\\n'" == r"""
    o  7 Added tag foo-bar for changeset fc281d8ff18d
    |
    o    6 merge 5 and 4
    |\
    | o  5 add another e
    | |
    o |  4 mv dir/b e
    |/
    @  3 mv a b; add d
    |
    o  2 mv b dir/b
    |
    o  1 copy a b
    |
    o  0 add a"""

# A template without trailing newline should do something sane

sh % "hg log -G -r '::2' --template '{rev} {desc}'" == r"""
    o  2 mv b dir/b
    |
    o  1 copy a b
    |
    o  0 add a"""

# Extra newlines must be preserved

sh % "hg log -G -r '::2' --template '\\n{rev} {desc}\\n\\n'" == r"""
    o
    |  2 mv b dir/b
    |
    o
    |  1 copy a b
    |
    o
       0 add a"""

# The almost-empty template should do something sane too ...

sh % "hg log -G -r '::2' --template '\\n'" == r"""
    o
    |
    o
    |
    o"""

# issue3772

sh % "hg log -G -r ':null'" == r"""
    o  changeset:   0:f8035bb17114
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     add a
    |
    o  changeset:   -1:000000000000
       user:
       date:        Thu Jan 01 00:00:00 1970 +0000"""
sh % "hg log -G -r 'null:null'" == r"""
    o  changeset:   -1:000000000000
       user:
       date:        Thu Jan 01 00:00:00 1970 +0000"""

# should not draw line down to null due to the magic of fullreposet

sh % "hg log -G -r 'all()'" | "tail -5" == r"""
    |
    o  changeset:   0:f8035bb17114
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     add a"""

# working-directory revision

sh % "hg log -G -qr '. + wdir()'" == r"""
    o  2147483647:ffffffffffff
    |
    @  3:5918b8d165d1
    |
    ~"""

# node template with changeset_printer:

sh % "hg log -Gqr '5:7' --config 'ui.graphnodetemplate=\"{rev}\"'" == r"""
    7  7:02dbb8e276b8
    |
    6    6:fc281d8ff18d
    |\
    | ~
    5  5:99b31f1c2782
    |
    ~"""

# node template with changeset_templater (shared cache variable):

sh % "hg log -Gr '5:7' -T '{latesttag % \"{rev} {tag}+{distance}\"}\\n' --config 'ui.graphnodetemplate={ifeq(latesttagdistance, 0, \"#\", graphnode)}'" == r"""
    o  7 foo-bar+1
    |
    #    6 foo-bar+0
    |\
    | ~
    o  5 null+5
    |
    ~"""

# label() should just work in node template:

sh % "hg log -Gqr 7 --config 'extensions.color=' '--color=debug' --config 'ui.graphnodetemplate={label(\"branch.{branch}\", rev)}'" == r"""
    [branch.default|7]  [log.node|7:02dbb8e276b8]
    |
    ~"""

sh % "cd .."

# change graph edge styling

sh % "cd repo"
sh % "cat" << r"""
[experimental]
graphstyle.parent = |
graphstyle.grandparent = :
graphstyle.missing =
""" >> "$HGRCPATH"
sh % "hg log -G -r 'file(\"a\")' -m" == r"""
    @  changeset:   36:95fa8febd08a
    :  parent:      35:9159c3644c5e
    :  parent:      35:9159c3644c5e
    :  user:        test
    :  date:        Thu Jan 01 00:00:36 1970 +0000
    :  summary:     (36) buggy merge: identical parents
    :
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    | :  parent:      31:621d83e11f67
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:32 1970 +0000
    | :  summary:     (32) expand
    | :
    o :  changeset:   31:621d83e11f67
    |\:  parent:      21:d42a756af44d
    | :  parent:      30:6e11cd4b648f
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:31 1970 +0000
    | :  summary:     (31) expand
    | :
    o :    changeset:   30:6e11cd4b648f
    |\ \   parent:      28:44ecd0b9ae99
    | ~ :  parent:      29:cd9bb2be7593
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:30 1970 +0000
    |   :  summary:     (30) expand
    |  /
    o :    changeset:   28:44ecd0b9ae99
    |\ \   parent:      1:6db2ef61d156
    | ~ :  parent:      26:7f25b6c2f0b9
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:28 1970 +0000
    |   :  summary:     (28) merge zero known
    |  /
    o :    changeset:   26:7f25b6c2f0b9
    |\ \   parent:      18:1aa84d96232a
    | | :  parent:      25:91da8ed57247
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:26 1970 +0000
    | | :  summary:     (26) merge one known; far right
    | | :
    | o :  changeset:   25:91da8ed57247
    | |\:  parent:      21:d42a756af44d
    | | :  parent:      24:a9c19a3d96b7
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:25 1970 +0000
    | | :  summary:     (25) merge one known; far left
    | | :
    | o :    changeset:   24:a9c19a3d96b7
    | |\ \   parent:      0:e6eb3150255d
    | | ~ :  parent:      23:a01cddf0766d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:24 1970 +0000
    | |   :  summary:     (24) merge one known; immediate right
    | |  /
    | o :    changeset:   23:a01cddf0766d
    | |\ \   parent:      1:6db2ef61d156
    | | ~ :  parent:      22:e0d9cccacb5d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:23 1970 +0000
    | |   :  summary:     (23) merge one known; immediate left
    | |  /
    | o :  changeset:   22:e0d9cccacb5d
    |/:/   parent:      18:1aa84d96232a
    | :    parent:      21:d42a756af44d
    | :    user:        test
    | :    date:        Thu Jan 01 00:00:22 1970 +0000
    | :    summary:     (22) merge two known; one far left, one far right
    | :
    | o    changeset:   21:d42a756af44d
    | |\   parent:      19:31ddc2c1573b
    | | |  parent:      20:d30ed6450e32
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | |  summary:     (21) expand
    | | |
    +---o  changeset:   20:d30ed6450e32
    | | |  parent:      0:e6eb3150255d
    | | ~  parent:      18:1aa84d96232a
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | |    summary:     (20) merge two known; two far right
    | |
    | o    changeset:   19:31ddc2c1573b
    | |\   parent:      15:1dda3f72782d
    | | |  parent:      17:44765d7c06e0
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | |  summary:     (19) expand
    | | |
    o | |  changeset:   18:1aa84d96232a
    |\| |  parent:      1:6db2ef61d156
    ~ | |  parent:      15:1dda3f72782d
      | |  user:        test
      | |  date:        Thu Jan 01 00:00:18 1970 +0000
      | |  summary:     (18) merge two known; two far left
     / /
    | o    changeset:   17:44765d7c06e0
    | |\   parent:      12:86b91144a6e9
    | | |  parent:      16:3677d192927d
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | |  summary:     (17) expand
    | | |
    | | o    changeset:   16:3677d192927d
    | | |\   parent:      0:e6eb3150255d
    | | ~ ~  parent:      1:6db2ef61d156
    | |      user:        test
    | |      date:        Thu Jan 01 00:00:16 1970 +0000
    | |      summary:     (16) merge two known; one immediate right, one near right
    | |
    o |    changeset:   15:1dda3f72782d
    |\ \   parent:      13:22d8966a97e3
    | | |  parent:      14:8eac370358ef
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | |  summary:     (15) expand
    | | |
    | o |  changeset:   14:8eac370358ef
    | |\|  parent:      0:e6eb3150255d
    | ~ |  parent:      12:86b91144a6e9
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:14 1970 +0000
    |   |  summary:     (14) merge two known; one immediate right, one far right
    |  /
    o |    changeset:   13:22d8966a97e3
    |\ \   parent:      9:7010c0af0a35
    | | |  parent:      11:832d76e6bdf2
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | |  summary:     (13) expand
    | | |
    +---o  changeset:   12:86b91144a6e9
    | | |  parent:      1:6db2ef61d156
    | | ~  parent:      9:7010c0af0a35
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | |    summary:     (12) merge two known; one immediate right, one far left
    | |
    | o    changeset:   11:832d76e6bdf2
    | |\   parent:      6:b105a072e251
    | | |  parent:      10:74c64d036d72
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | |  summary:     (11) expand
    | | |
    | | o  changeset:   10:74c64d036d72
    | |/|  parent:      0:e6eb3150255d
    | | ~  parent:      6:b105a072e251
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | |    summary:     (10) merge two known; one immediate left, one near right
    | |
    o |    changeset:   9:7010c0af0a35
    |\ \   parent:      7:b632bb1b1224
    | | |  parent:      8:7a0b11f71937
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | |  summary:     (9) expand
    | | |
    | o |  changeset:   8:7a0b11f71937
    |/| |  parent:      0:e6eb3150255d
    | ~ |  parent:      7:b632bb1b1224
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:08 1970 +0000
    |   |  summary:     (8) merge two known; one immediate left, one far right
    |  /
    o |    changeset:   7:b632bb1b1224
    |\ \   parent:      2:3d9a33b8d1e1
    | ~ |  parent:      5:4409d547b708
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:07 1970 +0000
    |   |  summary:     (7) expand
    |  /
    | o  changeset:   6:b105a072e251
    |/|  parent:      2:3d9a33b8d1e1
    | ~  parent:      5:4409d547b708
    |    user:        test
    |    date:        Thu Jan 01 00:00:06 1970 +0000
    |    summary:     (6) merge two known; one immediate left, one far left
    |
    o    changeset:   5:4409d547b708
    |\   parent:      3:27eef8ed80b4
    | ~  parent:      4:26a8bac39d9f
    |    user:        test
    |    date:        Thu Jan 01 00:00:05 1970 +0000
    |    summary:     (5) expand
    |
    o    changeset:   4:26a8bac39d9f
    |\   parent:      1:6db2ef61d156
    ~ ~  parent:      3:27eef8ed80b4
         user:        test
         date:        Thu Jan 01 00:00:04 1970 +0000
         summary:     (4) merge two known; one immediate left, one immediate right"""

# Setting HGPLAIN ignores graphmod styling:

sh % "'HGPLAIN=1' hg log -G -r 'file(\"a\")' -m" == r"""
    @  changeset:   36:95fa8febd08a
    |  parent:      35:9159c3644c5e
    |  parent:      35:9159c3644c5e
    |  user:        test
    |  date:        Thu Jan 01 00:00:36 1970 +0000
    |  summary:     (36) buggy merge: identical parents
    |
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    | |  parent:      31:621d83e11f67
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:32 1970 +0000
    | |  summary:     (32) expand
    | |
    o |  changeset:   31:621d83e11f67
    |\|  parent:      21:d42a756af44d
    | |  parent:      30:6e11cd4b648f
    | |  user:        test
    | |  date:        Thu Jan 01 00:00:31 1970 +0000
    | |  summary:     (31) expand
    | |
    o |    changeset:   30:6e11cd4b648f
    |\ \   parent:      28:44ecd0b9ae99
    | | |  parent:      29:cd9bb2be7593
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:30 1970 +0000
    | | |  summary:     (30) expand
    | | |
    o | |    changeset:   28:44ecd0b9ae99
    |\ \ \   parent:      1:6db2ef61d156
    | | | |  parent:      26:7f25b6c2f0b9
    | | | |  user:        test
    | | | |  date:        Thu Jan 01 00:00:28 1970 +0000
    | | | |  summary:     (28) merge zero known
    | | | |
    o | | |    changeset:   26:7f25b6c2f0b9
    |\ \ \ \   parent:      18:1aa84d96232a
    | | | | |  parent:      25:91da8ed57247
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:26 1970 +0000
    | | | | |  summary:     (26) merge one known; far right
    | | | | |
    | o-----+  changeset:   25:91da8ed57247
    | | | | |  parent:      21:d42a756af44d
    | | | | |  parent:      24:a9c19a3d96b7
    | | | | |  user:        test
    | | | | |  date:        Thu Jan 01 00:00:25 1970 +0000
    | | | | |  summary:     (25) merge one known; far left
    | | | | |
    | o | | |    changeset:   24:a9c19a3d96b7
    | |\ \ \ \   parent:      0:e6eb3150255d
    | | | | | |  parent:      23:a01cddf0766d
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:24 1970 +0000
    | | | | | |  summary:     (24) merge one known; immediate right
    | | | | | |
    | o---+ | |  changeset:   23:a01cddf0766d
    | | | | | |  parent:      1:6db2ef61d156
    | | | | | |  parent:      22:e0d9cccacb5d
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:23 1970 +0000
    | | | | | |  summary:     (23) merge one known; immediate left
    | | | | | |
    | o-------+  changeset:   22:e0d9cccacb5d
    | | | | | |  parent:      18:1aa84d96232a
    |/ / / / /   parent:      21:d42a756af44d
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:22 1970 +0000
    | | | | |    summary:     (22) merge two known; one far left, one far right
    | | | | |
    | | | | o    changeset:   21:d42a756af44d
    | | | | |\   parent:      19:31ddc2c1573b
    | | | | | |  parent:      20:d30ed6450e32
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | | | | |  summary:     (21) expand
    | | | | | |
    +-+-------o  changeset:   20:d30ed6450e32
    | | | | |    parent:      0:e6eb3150255d
    | | | | |    parent:      18:1aa84d96232a
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | | | | |    summary:     (20) merge two known; two far right
    | | | | |
    | | | | o    changeset:   19:31ddc2c1573b
    | | | | |\   parent:      15:1dda3f72782d
    | | | | | |  parent:      17:44765d7c06e0
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | | | | |  summary:     (19) expand
    | | | | | |
    o---+---+ |  changeset:   18:1aa84d96232a
      | | | | |  parent:      1:6db2ef61d156
     / / / / /   parent:      15:1dda3f72782d
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:18 1970 +0000
    | | | | |    summary:     (18) merge two known; two far left
    | | | | |
    | | | | o    changeset:   17:44765d7c06e0
    | | | | |\   parent:      12:86b91144a6e9
    | | | | | |  parent:      16:3677d192927d
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | | | | |  summary:     (17) expand
    | | | | | |
    +-+-------o  changeset:   16:3677d192927d
    | | | | |    parent:      0:e6eb3150255d
    | | | | |    parent:      1:6db2ef61d156
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:16 1970 +0000
    | | | | |    summary:     (16) merge two known; one immediate right, one near right
    | | | | |
    | | | o |    changeset:   15:1dda3f72782d
    | | | |\ \   parent:      13:22d8966a97e3
    | | | | | |  parent:      14:8eac370358ef
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | | | | |  summary:     (15) expand
    | | | | | |
    +-------o |  changeset:   14:8eac370358ef
    | | | | |/   parent:      0:e6eb3150255d
    | | | | |    parent:      12:86b91144a6e9
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:14 1970 +0000
    | | | | |    summary:     (14) merge two known; one immediate right, one far right
    | | | | |
    | | | o |    changeset:   13:22d8966a97e3
    | | | |\ \   parent:      9:7010c0af0a35
    | | | | | |  parent:      11:832d76e6bdf2
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | | | | |  summary:     (13) expand
    | | | | | |
    | +---+---o  changeset:   12:86b91144a6e9
    | | | | |    parent:      1:6db2ef61d156
    | | | | |    parent:      9:7010c0af0a35
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | | | | |    summary:     (12) merge two known; one immediate right, one far left
    | | | | |
    | | | | o    changeset:   11:832d76e6bdf2
    | | | | |\   parent:      6:b105a072e251
    | | | | | |  parent:      10:74c64d036d72
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | | | | |  summary:     (11) expand
    | | | | | |
    +---------o  changeset:   10:74c64d036d72
    | | | | |/   parent:      0:e6eb3150255d
    | | | | |    parent:      6:b105a072e251
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | | | | |    summary:     (10) merge two known; one immediate left, one near right
    | | | | |
    | | | o |    changeset:   9:7010c0af0a35
    | | | |\ \   parent:      7:b632bb1b1224
    | | | | | |  parent:      8:7a0b11f71937
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | | | | |  summary:     (9) expand
    | | | | | |
    +-------o |  changeset:   8:7a0b11f71937
    | | | |/ /   parent:      0:e6eb3150255d
    | | | | |    parent:      7:b632bb1b1224
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:08 1970 +0000
    | | | | |    summary:     (8) merge two known; one immediate left, one far right
    | | | | |
    | | | o |    changeset:   7:b632bb1b1224
    | | | |\ \   parent:      2:3d9a33b8d1e1
    | | | | | |  parent:      5:4409d547b708
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:07 1970 +0000
    | | | | | |  summary:     (7) expand
    | | | | | |
    | | | +---o  changeset:   6:b105a072e251
    | | | | |/   parent:      2:3d9a33b8d1e1
    | | | | |    parent:      5:4409d547b708
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:06 1970 +0000
    | | | | |    summary:     (6) merge two known; one immediate left, one far left
    | | | | |
    | | | o |    changeset:   5:4409d547b708
    | | | |\ \   parent:      3:27eef8ed80b4
    | | | | | |  parent:      4:26a8bac39d9f
    | | | | | |  user:        test
    | | | | | |  date:        Thu Jan 01 00:00:05 1970 +0000
    | | | | | |  summary:     (5) expand
    | | | | | |
    | +---o | |  changeset:   4:26a8bac39d9f
    | | | |/ /   parent:      1:6db2ef61d156
    | | | | |    parent:      3:27eef8ed80b4
    | | | | |    user:        test
    | | | | |    date:        Thu Jan 01 00:00:04 1970 +0000
    | | | | |    summary:     (4) merge two known; one immediate left, one immediate right
    | | | | |"""

# .. unless HGPLAINEXCEPT=graph is set:

sh % "'HGPLAIN=1' 'HGPLAINEXCEPT=graph' hg log -G -r 'file(\"a\")' -m" == r"""
    @  changeset:   36:95fa8febd08a
    :  parent:      35:9159c3644c5e
    :  parent:      35:9159c3644c5e
    :  user:        test
    :  date:        Thu Jan 01 00:00:36 1970 +0000
    :  summary:     (36) buggy merge: identical parents
    :
    o    changeset:   32:d06dffa21a31
    |\   parent:      27:886ed638191b
    | :  parent:      31:621d83e11f67
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:32 1970 +0000
    | :  summary:     (32) expand
    | :
    o :  changeset:   31:621d83e11f67
    |\:  parent:      21:d42a756af44d
    | :  parent:      30:6e11cd4b648f
    | :  user:        test
    | :  date:        Thu Jan 01 00:00:31 1970 +0000
    | :  summary:     (31) expand
    | :
    o :    changeset:   30:6e11cd4b648f
    |\ \   parent:      28:44ecd0b9ae99
    | ~ :  parent:      29:cd9bb2be7593
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:30 1970 +0000
    |   :  summary:     (30) expand
    |  /
    o :    changeset:   28:44ecd0b9ae99
    |\ \   parent:      1:6db2ef61d156
    | ~ :  parent:      26:7f25b6c2f0b9
    |   :  user:        test
    |   :  date:        Thu Jan 01 00:00:28 1970 +0000
    |   :  summary:     (28) merge zero known
    |  /
    o :    changeset:   26:7f25b6c2f0b9
    |\ \   parent:      18:1aa84d96232a
    | | :  parent:      25:91da8ed57247
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:26 1970 +0000
    | | :  summary:     (26) merge one known; far right
    | | :
    | o :  changeset:   25:91da8ed57247
    | |\:  parent:      21:d42a756af44d
    | | :  parent:      24:a9c19a3d96b7
    | | :  user:        test
    | | :  date:        Thu Jan 01 00:00:25 1970 +0000
    | | :  summary:     (25) merge one known; far left
    | | :
    | o :    changeset:   24:a9c19a3d96b7
    | |\ \   parent:      0:e6eb3150255d
    | | ~ :  parent:      23:a01cddf0766d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:24 1970 +0000
    | |   :  summary:     (24) merge one known; immediate right
    | |  /
    | o :    changeset:   23:a01cddf0766d
    | |\ \   parent:      1:6db2ef61d156
    | | ~ :  parent:      22:e0d9cccacb5d
    | |   :  user:        test
    | |   :  date:        Thu Jan 01 00:00:23 1970 +0000
    | |   :  summary:     (23) merge one known; immediate left
    | |  /
    | o :  changeset:   22:e0d9cccacb5d
    |/:/   parent:      18:1aa84d96232a
    | :    parent:      21:d42a756af44d
    | :    user:        test
    | :    date:        Thu Jan 01 00:00:22 1970 +0000
    | :    summary:     (22) merge two known; one far left, one far right
    | :
    | o    changeset:   21:d42a756af44d
    | |\   parent:      19:31ddc2c1573b
    | | |  parent:      20:d30ed6450e32
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:21 1970 +0000
    | | |  summary:     (21) expand
    | | |
    +---o  changeset:   20:d30ed6450e32
    | | |  parent:      0:e6eb3150255d
    | | ~  parent:      18:1aa84d96232a
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:20 1970 +0000
    | |    summary:     (20) merge two known; two far right
    | |
    | o    changeset:   19:31ddc2c1573b
    | |\   parent:      15:1dda3f72782d
    | | |  parent:      17:44765d7c06e0
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:19 1970 +0000
    | | |  summary:     (19) expand
    | | |
    o | |  changeset:   18:1aa84d96232a
    |\| |  parent:      1:6db2ef61d156
    ~ | |  parent:      15:1dda3f72782d
      | |  user:        test
      | |  date:        Thu Jan 01 00:00:18 1970 +0000
      | |  summary:     (18) merge two known; two far left
     / /
    | o    changeset:   17:44765d7c06e0
    | |\   parent:      12:86b91144a6e9
    | | |  parent:      16:3677d192927d
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:17 1970 +0000
    | | |  summary:     (17) expand
    | | |
    | | o    changeset:   16:3677d192927d
    | | |\   parent:      0:e6eb3150255d
    | | ~ ~  parent:      1:6db2ef61d156
    | |      user:        test
    | |      date:        Thu Jan 01 00:00:16 1970 +0000
    | |      summary:     (16) merge two known; one immediate right, one near right
    | |
    o |    changeset:   15:1dda3f72782d
    |\ \   parent:      13:22d8966a97e3
    | | |  parent:      14:8eac370358ef
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:15 1970 +0000
    | | |  summary:     (15) expand
    | | |
    | o |  changeset:   14:8eac370358ef
    | |\|  parent:      0:e6eb3150255d
    | ~ |  parent:      12:86b91144a6e9
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:14 1970 +0000
    |   |  summary:     (14) merge two known; one immediate right, one far right
    |  /
    o |    changeset:   13:22d8966a97e3
    |\ \   parent:      9:7010c0af0a35
    | | |  parent:      11:832d76e6bdf2
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:13 1970 +0000
    | | |  summary:     (13) expand
    | | |
    +---o  changeset:   12:86b91144a6e9
    | | |  parent:      1:6db2ef61d156
    | | ~  parent:      9:7010c0af0a35
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:12 1970 +0000
    | |    summary:     (12) merge two known; one immediate right, one far left
    | |
    | o    changeset:   11:832d76e6bdf2
    | |\   parent:      6:b105a072e251
    | | |  parent:      10:74c64d036d72
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:11 1970 +0000
    | | |  summary:     (11) expand
    | | |
    | | o  changeset:   10:74c64d036d72
    | |/|  parent:      0:e6eb3150255d
    | | ~  parent:      6:b105a072e251
    | |    user:        test
    | |    date:        Thu Jan 01 00:00:10 1970 +0000
    | |    summary:     (10) merge two known; one immediate left, one near right
    | |
    o |    changeset:   9:7010c0af0a35
    |\ \   parent:      7:b632bb1b1224
    | | |  parent:      8:7a0b11f71937
    | | |  user:        test
    | | |  date:        Thu Jan 01 00:00:09 1970 +0000
    | | |  summary:     (9) expand
    | | |
    | o |  changeset:   8:7a0b11f71937
    |/| |  parent:      0:e6eb3150255d
    | ~ |  parent:      7:b632bb1b1224
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:08 1970 +0000
    |   |  summary:     (8) merge two known; one immediate left, one far right
    |  /
    o |    changeset:   7:b632bb1b1224
    |\ \   parent:      2:3d9a33b8d1e1
    | ~ |  parent:      5:4409d547b708
    |   |  user:        test
    |   |  date:        Thu Jan 01 00:00:07 1970 +0000
    |   |  summary:     (7) expand
    |  /
    | o  changeset:   6:b105a072e251
    |/|  parent:      2:3d9a33b8d1e1
    | ~  parent:      5:4409d547b708
    |    user:        test
    |    date:        Thu Jan 01 00:00:06 1970 +0000
    |    summary:     (6) merge two known; one immediate left, one far left
    |
    o    changeset:   5:4409d547b708
    |\   parent:      3:27eef8ed80b4
    | ~  parent:      4:26a8bac39d9f
    |    user:        test
    |    date:        Thu Jan 01 00:00:05 1970 +0000
    |    summary:     (5) expand
    |
    o    changeset:   4:26a8bac39d9f
    |\   parent:      1:6db2ef61d156
    ~ ~  parent:      3:27eef8ed80b4
         user:        test
         date:        Thu Jan 01 00:00:04 1970 +0000
         summary:     (4) merge two known; one immediate left, one immediate right"""
# Draw only part of a grandparent line differently with "<N><char>"; only the
# last N lines (for positive N) or everything but the first N lines (for
# negative N) along the current node use the style, the rest of the edge uses
# the parent edge styling.

# Last 3 lines:

sh % "cat" << r"""
[experimental]
graphstyle.parent = !
graphstyle.grandparent = 3.
graphstyle.missing =
""" >> "$HGRCPATH"
sh % "hg log -G -r '36:18 & file(\"a\")' -m" == r"""
    @  changeset:   36:95fa8febd08a
    !  parent:      35:9159c3644c5e
    !  parent:      35:9159c3644c5e
    !  user:        test
    .  date:        Thu Jan 01 00:00:36 1970 +0000
    .  summary:     (36) buggy merge: identical parents
    .
    o    changeset:   32:d06dffa21a31
    !\   parent:      27:886ed638191b
    ! !  parent:      31:621d83e11f67
    ! !  user:        test
    ! .  date:        Thu Jan 01 00:00:32 1970 +0000
    ! .  summary:     (32) expand
    ! .
    o !  changeset:   31:621d83e11f67
    !\!  parent:      21:d42a756af44d
    ! !  parent:      30:6e11cd4b648f
    ! !  user:        test
    ! !  date:        Thu Jan 01 00:00:31 1970 +0000
    ! !  summary:     (31) expand
    ! !
    o !    changeset:   30:6e11cd4b648f
    !\ \   parent:      28:44ecd0b9ae99
    ! ~ !  parent:      29:cd9bb2be7593
    !   !  user:        test
    !   !  date:        Thu Jan 01 00:00:30 1970 +0000
    !   !  summary:     (30) expand
    !  /
    o !    changeset:   28:44ecd0b9ae99
    !\ \   parent:      1:6db2ef61d156
    ! ~ !  parent:      26:7f25b6c2f0b9
    !   !  user:        test
    !   !  date:        Thu Jan 01 00:00:28 1970 +0000
    !   !  summary:     (28) merge zero known
    !  /
    o !    changeset:   26:7f25b6c2f0b9
    !\ \   parent:      18:1aa84d96232a
    ! ! !  parent:      25:91da8ed57247
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:26 1970 +0000
    ! ! !  summary:     (26) merge one known; far right
    ! ! !
    ! o !  changeset:   25:91da8ed57247
    ! !\!  parent:      21:d42a756af44d
    ! ! !  parent:      24:a9c19a3d96b7
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:25 1970 +0000
    ! ! !  summary:     (25) merge one known; far left
    ! ! !
    ! o !    changeset:   24:a9c19a3d96b7
    ! !\ \   parent:      0:e6eb3150255d
    ! ! ~ !  parent:      23:a01cddf0766d
    ! !   !  user:        test
    ! !   !  date:        Thu Jan 01 00:00:24 1970 +0000
    ! !   !  summary:     (24) merge one known; immediate right
    ! !  /
    ! o !    changeset:   23:a01cddf0766d
    ! !\ \   parent:      1:6db2ef61d156
    ! ! ~ !  parent:      22:e0d9cccacb5d
    ! !   !  user:        test
    ! !   !  date:        Thu Jan 01 00:00:23 1970 +0000
    ! !   !  summary:     (23) merge one known; immediate left
    ! !  /
    ! o !  changeset:   22:e0d9cccacb5d
    !/!/   parent:      18:1aa84d96232a
    ! !    parent:      21:d42a756af44d
    ! !    user:        test
    ! !    date:        Thu Jan 01 00:00:22 1970 +0000
    ! !    summary:     (22) merge two known; one far left, one far right
    ! !
    ! o    changeset:   21:d42a756af44d
    ! !\   parent:      19:31ddc2c1573b
    ! ! !  parent:      20:d30ed6450e32
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:21 1970 +0000
    ! ! !  summary:     (21) expand
    ! ! !
    +---o  changeset:   20:d30ed6450e32
    ! ! |  parent:      0:e6eb3150255d
    ! ! ~  parent:      18:1aa84d96232a
    ! !    user:        test
    ! !    date:        Thu Jan 01 00:00:20 1970 +0000
    ! !    summary:     (20) merge two known; two far right
    ! !
    ! o    changeset:   19:31ddc2c1573b
    ! |\   parent:      15:1dda3f72782d
    ! ~ ~  parent:      17:44765d7c06e0
    !      user:        test
    !      date:        Thu Jan 01 00:00:19 1970 +0000
    !      summary:     (19) expand
    !
    o    changeset:   18:1aa84d96232a
    |\   parent:      1:6db2ef61d156
    ~ ~  parent:      15:1dda3f72782d
         user:        test
         date:        Thu Jan 01 00:00:18 1970 +0000
         summary:     (18) merge two known; two far left"""
# All but the first 3 lines:

sh % "cat" << r"""
[experimental]
graphstyle.parent = !
graphstyle.grandparent = -3.
graphstyle.missing =
""" >> "$HGRCPATH"
sh % "hg log -G -r '36:18 & file(\"a\")' -m" == r"""
    @  changeset:   36:95fa8febd08a
    !  parent:      35:9159c3644c5e
    !  parent:      35:9159c3644c5e
    .  user:        test
    .  date:        Thu Jan 01 00:00:36 1970 +0000
    .  summary:     (36) buggy merge: identical parents
    .
    o    changeset:   32:d06dffa21a31
    !\   parent:      27:886ed638191b
    ! !  parent:      31:621d83e11f67
    ! .  user:        test
    ! .  date:        Thu Jan 01 00:00:32 1970 +0000
    ! .  summary:     (32) expand
    ! .
    o !  changeset:   31:621d83e11f67
    !\!  parent:      21:d42a756af44d
    ! !  parent:      30:6e11cd4b648f
    ! !  user:        test
    ! !  date:        Thu Jan 01 00:00:31 1970 +0000
    ! !  summary:     (31) expand
    ! !
    o !    changeset:   30:6e11cd4b648f
    !\ \   parent:      28:44ecd0b9ae99
    ! ~ !  parent:      29:cd9bb2be7593
    !   !  user:        test
    !   !  date:        Thu Jan 01 00:00:30 1970 +0000
    !   !  summary:     (30) expand
    !  /
    o !    changeset:   28:44ecd0b9ae99
    !\ \   parent:      1:6db2ef61d156
    ! ~ !  parent:      26:7f25b6c2f0b9
    !   !  user:        test
    !   !  date:        Thu Jan 01 00:00:28 1970 +0000
    !   !  summary:     (28) merge zero known
    !  /
    o !    changeset:   26:7f25b6c2f0b9
    !\ \   parent:      18:1aa84d96232a
    ! ! !  parent:      25:91da8ed57247
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:26 1970 +0000
    ! ! !  summary:     (26) merge one known; far right
    ! ! !
    ! o !  changeset:   25:91da8ed57247
    ! !\!  parent:      21:d42a756af44d
    ! ! !  parent:      24:a9c19a3d96b7
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:25 1970 +0000
    ! ! !  summary:     (25) merge one known; far left
    ! ! !
    ! o !    changeset:   24:a9c19a3d96b7
    ! !\ \   parent:      0:e6eb3150255d
    ! ! ~ !  parent:      23:a01cddf0766d
    ! !   !  user:        test
    ! !   !  date:        Thu Jan 01 00:00:24 1970 +0000
    ! !   !  summary:     (24) merge one known; immediate right
    ! !  /
    ! o !    changeset:   23:a01cddf0766d
    ! !\ \   parent:      1:6db2ef61d156
    ! ! ~ !  parent:      22:e0d9cccacb5d
    ! !   !  user:        test
    ! !   !  date:        Thu Jan 01 00:00:23 1970 +0000
    ! !   !  summary:     (23) merge one known; immediate left
    ! !  /
    ! o !  changeset:   22:e0d9cccacb5d
    !/!/   parent:      18:1aa84d96232a
    ! !    parent:      21:d42a756af44d
    ! !    user:        test
    ! !    date:        Thu Jan 01 00:00:22 1970 +0000
    ! !    summary:     (22) merge two known; one far left, one far right
    ! !
    ! o    changeset:   21:d42a756af44d
    ! !\   parent:      19:31ddc2c1573b
    ! ! !  parent:      20:d30ed6450e32
    ! ! !  user:        test
    ! ! !  date:        Thu Jan 01 00:00:21 1970 +0000
    ! ! !  summary:     (21) expand
    ! ! !
    +---o  changeset:   20:d30ed6450e32
    ! ! |  parent:      0:e6eb3150255d
    ! ! ~  parent:      18:1aa84d96232a
    ! !    user:        test
    ! !    date:        Thu Jan 01 00:00:20 1970 +0000
    ! !    summary:     (20) merge two known; two far right
    ! !
    ! o    changeset:   19:31ddc2c1573b
    ! |\   parent:      15:1dda3f72782d
    ! ~ ~  parent:      17:44765d7c06e0
    !      user:        test
    !      date:        Thu Jan 01 00:00:19 1970 +0000
    !      summary:     (19) expand
    !
    o    changeset:   18:1aa84d96232a
    |\   parent:      1:6db2ef61d156
    ~ ~  parent:      15:1dda3f72782d
         user:        test
         date:        Thu Jan 01 00:00:18 1970 +0000
         summary:     (18) merge two known; two far left"""
sh % "cd .."

# Change graph shorten, test better with graphstyle.missing not none

sh % "cd repo"
sh % "cat" << r"""
[experimental]
graphstyle.parent = |
graphstyle.grandparent = :
graphstyle.missing = '
graphshorten = true
""" >> "$HGRCPATH"
sh % "hg log -G -r 'file(\"a\")' -m -T '{rev} {desc}'" == r"""
    @  36 (36) buggy merge: identical parents
    o    32 (32) expand
    |\
    o :  31 (31) expand
    |\:
    o :    30 (30) expand
    |\ \
    o \ \    28 (28) merge zero known
    |\ \ \
    o \ \ \    26 (26) merge one known; far right
    |\ \ \ \
    | o-----+  25 (25) merge one known; far left
    | o ' ' :    24 (24) merge one known; immediate right
    | |\ \ \ \
    | o---+ ' :  23 (23) merge one known; immediate left
    | o-------+  22 (22) merge two known; one far left, one far right
    |/ / / / /
    | ' ' ' o    21 (21) expand
    | ' ' ' |\
    +-+-------o  20 (20) merge two known; two far right
    | ' ' ' o    19 (19) expand
    | ' ' ' |\
    o---+---+ |  18 (18) merge two known; two far left
     / / / / /
    ' ' ' | o    17 (17) expand
    ' ' ' | |\
    +-+-------o  16 (16) merge two known; one immediate right, one near right
    ' ' ' o |    15 (15) expand
    ' ' ' |\ \
    +-------o |  14 (14) merge two known; one immediate right, one far right
    ' ' ' | |/
    ' ' ' o |    13 (13) expand
    ' ' ' |\ \
    ' +---+---o  12 (12) merge two known; one immediate right, one far left
    ' ' ' | o    11 (11) expand
    ' ' ' | |\
    +---------o  10 (10) merge two known; one immediate left, one near right
    ' ' ' | |/
    ' ' ' o |    9 (9) expand
    ' ' ' |\ \
    +-------o |  8 (8) merge two known; one immediate left, one far right
    ' ' ' |/ /
    ' ' ' o |    7 (7) expand
    ' ' ' |\ \
    ' ' ' +---o  6 (6) merge two known; one immediate left, one far left
    ' ' ' | '/
    ' ' ' o '    5 (5) expand
    ' ' ' |\ \
    ' +---o ' '  4 (4) merge two known; one immediate left, one immediate right
    ' ' ' '/ /"""

# behavior with newlines

sh % "hg log -G -r '::2' -T '{rev} {desc}'" == r"""
    o  2 (2) collapse
    o  1 (1) collapse
    o  0 (0) root"""

sh % "hg log -G -r '::2' -T '{rev} {desc}\\n'" == r"""
    o  2 (2) collapse
    o  1 (1) collapse
    o  0 (0) root"""

sh % "hg log -G -r '::2' -T '{rev} {desc}\\n\\n'" == r"""
    o  2 (2) collapse
    |
    o  1 (1) collapse
    |
    o  0 (0) root"""

sh % "hg log -G -r '::2' -T '\\n{rev} {desc}'" == r"""
    o
    |  2 (2) collapse
    o
    |  1 (1) collapse
    o
       0 (0) root"""

sh % "hg log -G -r '::2' -T '{rev} {desc}\\n\\n\\n'" == r"""
    o  2 (2) collapse
    |
    |
    o  1 (1) collapse
    |
    |
    o  0 (0) root"""
sh % "cd .."

# When inserting extra line nodes to handle more than 2 parents, ensure that
# the right node styles are used (issue5174):

sh % "hg init repo-issue5174"
sh % "cd repo-issue5174"
sh % "echo a" > "f0"
sh % "hg ci -Aqm 0"
sh % "echo a" > "f1"
sh % "hg ci -Aqm 1"
sh % "echo a" > "f2"
sh % "hg ci -Aqm 2"
sh % "hg co '.^'" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo a" > "f3"
sh % "hg ci -Aqm 3"
sh % "hg co '.^^'" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo a" > "f4"
sh % "hg ci -Aqm 4"
sh % "hg merge -r 2" == r"""
    2 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg ci -qm 5"
sh % "hg merge -r 3" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg ci -qm 6"
sh % "hg log -G -r '0 | 1 | 2 | 6'" == r"""
    @    changeset:   6:851fe89689ad
    :\   parent:      5:4f1e3cf15f5d
    : :  parent:      3:b74ba7084d2d
    : :  user:        test
    : :  date:        Thu Jan 01 00:00:00 1970 +0000
    : :  summary:     6
    : :
    : \
    : :\
    : o :  changeset:   2:3e6599df4cce
    : :/   user:        test
    : :    date:        Thu Jan 01 00:00:00 1970 +0000
    : :    summary:     2
    : :
    : o  changeset:   1:bd9a55143933
    :/   user:        test
    :    date:        Thu Jan 01 00:00:00 1970 +0000
    :    summary:     1
    :
    o  changeset:   0:870a5edc339c
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     0"""

sh % "cd .."

# Multiple roots (issue5440):

sh % "hg init multiroots"
sh % "cd multiroots"
sh % "cat" << r"""
[ui]
logtemplate = '{rev} {desc}\n\n'
""" > ".hg/hgrc"

sh % "touch foo"
sh % "hg ci -Aqm foo"
sh % "hg co -q null"
sh % "touch bar"
sh % "hg ci -Aqm bar"

sh % "hg log -Gr 'null:'" == r"""
    @  1 bar
    |
    | o  0 foo
    |/
    o  -1"""
sh % "hg log -Gr null+0" == r"""
    o  0 foo
    |
    o  -1"""
sh % "hg log -Gr null+1" == r"""
    @  1 bar
    |
    o  -1"""

sh % "cd .."
