# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

sh % "setconfig 'ui.allowemptycommit=1'"
sh % "enable convert"

sh % "hg init t"
sh % "cd t"
sh % "echo a" >> "a"
sh % "hg ci -qAm a0 -d '1 0'"
sh % "echo a" >> "a"
sh % "hg ci -m a1 -d '2 0'"
sh % "echo a" >> "a"
sh % "hg ci -m a2 -d '3 0'"
sh % "echo a" >> "a"
sh % "hg ci -m a3 -d '4 0'"
sh % "hg book -i brancha"

sh % "hg up -Cq 0"
sh % "echo b" >> "b"
sh % "hg ci -qAm b0 -d '6 0'"
sh % "hg book -i branchb"

sh % "hg up -qC brancha"
sh % "echo a" >> "a"
sh % "hg ci -m a4 -d '5 0'"
sh % "echo a" >> "a"
sh % "hg ci -m a5 -d '7 0'"
sh % "echo a" >> "a"
sh % "hg ci -m a6 -d '8 0'"

sh % "hg up -qC branchb"
sh % "echo b" >> "b"
sh % "hg ci -m b1 -d '9 0'"

sh % "hg up -qC 0"
sh % "echo c" >> "c"
sh % "hg ci -qAm c0 -d '10 0'"
sh % "hg bookmark branchc"

sh % "hg up -qC brancha"
sh % "hg ci -qm a7x -d '11 0'"

sh % "hg up -qC branchb"
sh % "hg ci -m b2x -d '12 0'"

sh % "hg up -qC branchc"
sh % "hg merge branchb -q"

sh % "hg ci -m c1 -d '13 0'"
sh % "hg bookmark -d brancha branchb branchc"
sh % "cd '$TESTTMP'"

# convert with datesort

sh % "hg convert --datesort t t-datesort" == r"""
    initializing destination t-datesort repository
    scanning source...
    sorting...
    converting...
    12 a0
    11 a1
    10 a2
    9 a3
    8 a4
    7 b0
    6 a5
    5 a6
    4 b1
    3 c0
    2 a7x
    1 b2x
    0 c1"""

# graph converted repo

sh % "hg -R t-datesort log -G --template '{rev} \"{desc}\"\\n'" == r'''
    o    12 "c1"
    |\
    | o  11 "b2x"
    | |
    | | o  10 "a7x"
    | | |
    o | |  9 "c0"
    | | |
    | o |  8 "b1"
    | | |
    | | o  7 "a6"
    | | |
    | | o  6 "a5"
    | | |
    | o |  5 "b0"
    |/ /
    | o  4 "a4"
    | |
    | o  3 "a3"
    | |
    | o  2 "a2"
    | |
    | o  1 "a1"
    |/
    o  0 "a0"'''

# convert with datesort (default mode)

sh % "hg convert t t-sourcesort" == r"""
    initializing destination t-sourcesort repository
    scanning source...
    sorting...
    converting...
    12 a0
    11 a1
    10 a2
    9 a3
    8 b0
    7 a4
    6 a5
    5 a6
    4 b1
    3 c0
    2 a7x
    1 b2x
    0 c1"""

# graph converted repo

sh % "hg -R t-sourcesort log -G --template '{rev} \"{desc}\"\\n'" == r'''
    o    12 "c1"
    |\
    | o  11 "b2x"
    | |
    | | o  10 "a7x"
    | | |
    o | |  9 "c0"
    | | |
    | o |  8 "b1"
    | | |
    | | o  7 "a6"
    | | |
    | | o  6 "a5"
    | | |
    | | o  5 "a4"
    | | |
    | o |  4 "b0"
    |/ /
    | o  3 "a3"
    | |
    | o  2 "a2"
    | |
    | o  1 "a1"
    |/
    o  0 "a0"'''

# convert with closesort

sh % "hg convert --closesort t t-closesort" == r"""
    initializing destination t-closesort repository
    scanning source...
    sorting...
    converting...
    12 a0
    11 a1
    10 a2
    9 a3
    8 b0
    7 a4
    6 a5
    5 a6
    4 b1
    3 c0
    2 a7x
    1 b2x
    0 c1"""

# graph converted repo

sh % "hg -R t-closesort log -G --template '{rev} \"{desc}\"\\n'" == r'''
    o    12 "c1"
    |\
    | o  11 "b2x"
    | |
    | | o  10 "a7x"
    | | |
    o | |  9 "c0"
    | | |
    | o |  8 "b1"
    | | |
    | | o  7 "a6"
    | | |
    | | o  6 "a5"
    | | |
    | | o  5 "a4"
    | | |
    | o |  4 "b0"
    |/ /
    | o  3 "a3"
    | |
    | o  2 "a2"
    | |
    | o  1 "a1"
    |/
    o  0 "a0"'''
