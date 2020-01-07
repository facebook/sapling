# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# plain

sh % "hg init"
sh % "hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' --config 'extensions.progress=' --config 'progress.debug=true'" == r"""
    progress: building: 0/12 revisions (0.00%)
    progress: building: 1/12 revisions (8.33%)
    progress: building: 1/12 revisions (8.33%)
    progress: building: 2/12 revisions (16.67%)
    progress: building: 3/12 revisions (25.00%)
    progress: building: 4/12 revisions (33.33%)
    progress: building: 4/12 revisions (33.33%)
    progress: building: 4/12 revisions (33.33%)
    progress: building: 5/12 revisions (41.67%)
    progress: building: 6/12 revisions (50.00%)
    progress: building: 7/12 revisions (58.33%)
    progress: building: 8/12 revisions (66.67%)
    progress: building: 8/12 revisions (66.67%)
    progress: building: 9/12 revisions (75.00%)
    progress: building: 10/12 revisions (83.33%)
    progress: building: 11/12 revisions (91.67%)
    progress: building (end)"""

# dag
sh % "hg debugdag --bookmarks -b" == r"""
    +2:f
    +3:p2
    @temp*f+3
    @default*/p2+2"""
# tip
sh % "hg id" == "000000000000"
# glog
sh % "hg log -G --template '{rev}: {desc} [{branches}] @ {date}\\n'" == r"""
    o  11: r11 [] @ 11.00
    |
    o  10: r10 [] @ 10.00
    |
    o    9: r9 [] @ 9.00
    |\
    | o  8: r8 [temp] @ 8.00
    | |
    | o  7: r7 [temp] @ 7.00
    | |
    | o  6: r6 [temp] @ 6.00
    | |
    | o  5: r5 [temp] @ 5.00
    | |
    o |  4: r4 [] @ 4.00
    | |
    o |  3: r3 [] @ 3.00
    | |
    o |  2: r2 [] @ 2.00
    |/
    o  1: r1 [] @ 1.00
    |
    o  0: r0 [] @ 0.00"""

# overwritten files, starting on a non-default branch

sh % "rm -r .hg"
sh % "hg init"
sh % "hg debugbuilddag '@start.@default.:f +3:p2 @temp <f+4 @default /p2 +2' -q -o"
# dag
sh % "hg debugdag --bookmarks -b" == r"""
    @start+1
    @default+1:f
    +3:p2
    @temp*f+3
    @default*/p2+2"""
# tip
sh % "hg id" == "000000000000"
# glog
sh % "hg log -G --template '{rev}: {desc} [{branches}] @ {date}\\n'" == r"""
    o  11: r11 [] @ 11.00
    |
    o  10: r10 [] @ 10.00
    |
    o    9: r9 [] @ 9.00
    |\
    | o  8: r8 [temp] @ 8.00
    | |
    | o  7: r7 [temp] @ 7.00
    | |
    | o  6: r6 [temp] @ 6.00
    | |
    | o  5: r5 [temp] @ 5.00
    | |
    o |  4: r4 [] @ 4.00
    | |
    o |  3: r3 [] @ 3.00
    | |
    o |  2: r2 [] @ 2.00
    |/
    o  1: r1 [] @ 1.00
    |
    o  0: r0 [start] @ 0.00"""
# glog of
sh % "hg log -G --template '{rev}: {desc} [{branches}]\\n' of" == r"""
    o  11: r11 []
    |
    o  10: r10 []
    |
    o    9: r9 []
    |\
    | o  8: r8 [temp]
    | |
    | o  7: r7 [temp]
    | |
    | o  6: r6 [temp]
    | |
    | o  5: r5 [temp]
    | |
    o |  4: r4 []
    | |
    o |  3: r3 []
    | |
    o |  2: r2 []
    |/
    o  1: r1 []
    |
    o  0: r0 [start]"""
# cat of
sh % "hg cat of --rev tip" == "r11"


# new and mergeable files

sh % "rm -r .hg"
sh % "hg init"
sh % "hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' -q -mn"
# dag
sh % "hg debugdag --bookmarks -b" == r"""
    +2:f
    +3:p2
    @temp*f+3
    @default*/p2+2"""
# tip
sh % "hg id" == "000000000000"
# glog
sh % "hg log -G --template '{rev}: {desc} [{branches}] @ {date}\\n'" == r"""
    o  11: r11 [] @ 11.00
    |
    o  10: r10 [] @ 10.00
    |
    o    9: r9 [] @ 9.00
    |\
    | o  8: r8 [temp] @ 8.00
    | |
    | o  7: r7 [temp] @ 7.00
    | |
    | o  6: r6 [temp] @ 6.00
    | |
    | o  5: r5 [temp] @ 5.00
    | |
    o |  4: r4 [] @ 4.00
    | |
    o |  3: r3 [] @ 3.00
    | |
    o |  2: r2 [] @ 2.00
    |/
    o  1: r1 [] @ 1.00
    |
    o  0: r0 [] @ 0.00"""
# glog mf
sh % "hg log -G --template '{rev}: {desc} [{branches}]\\n' mf" == r"""
    o  11: r11 []
    |
    o  10: r10 []
    |
    o    9: r9 []
    |\
    | o  8: r8 [temp]
    | |
    | o  7: r7 [temp]
    | |
    | o  6: r6 [temp]
    | |
    | o  5: r5 [temp]
    | |
    o |  4: r4 []
    | |
    o |  3: r3 []
    | |
    o |  2: r2 []
    |/
    o  1: r1 []
    |
    o  0: r0 []"""

# man r4
sh % "hg manifest -r4" == r"""
    mf
    nf0
    nf1
    nf2
    nf3
    nf4"""
# cat r4 mf
sh % "hg cat -r4 mf" == r"""
    0 r0
    1
    2 r1
    3
    4 r2
    5
    6 r3
    7
    8 r4
    9
    10
    11
    12
    13
    14
    15
    16
    17
    18
    19
    20
    21
    22
    23"""
# man r8
sh % "hg manifest -r8" == r"""
    mf
    nf0
    nf1
    nf5
    nf6
    nf7
    nf8"""
# cat r8 mf
sh % "hg cat -r8 mf" == r"""
    0 r0
    1
    2 r1
    3
    4
    5
    6
    7
    8
    9
    10 r5
    11
    12 r6
    13
    14 r7
    15
    16 r8
    17
    18
    19
    20
    21
    22
    23"""
# man
sh % "hg manifest --rev tip" == r"""
    mf
    nf0
    nf1
    nf10
    nf11
    nf2
    nf3
    nf4
    nf5
    nf6
    nf7
    nf8
    nf9"""
# cat mf
sh % "hg cat mf --rev tip" == r"""
    0 r0
    1
    2 r1
    3
    4 r2
    5
    6 r3
    7
    8 r4
    9
    10 r5
    11
    12 r6
    13
    14 r7
    15
    16 r8
    17
    18 r9
    19
    20 r10
    21
    22 r11
    23"""
