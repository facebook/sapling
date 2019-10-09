# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, shlib, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

# Set up test environment.


def mkcommit(name):
    open(name, "wb").write("%s\n" % name)
    sh.hg("ci", "-m", "add %s" % name, "-A", name)


shlib.mkcommit = mkcommit


sh % "enable amend rebase"
sh % "setconfig experimental.evolution.allowdivergence=True"
sh % 'setconfig "experimental.evolution=createmarkers, allowunstable"'
sh % "hg init restack"
sh % "cd restack"

# Note: Repositories populated by `hg debugbuilddag` don't seem to
# correctly show all commits in the log output. Manually creating the
# commits results in the expected behavior, so commits are manually
# created in the test cases below.

# Test unsupported flags:
sh % "hg rebase --restack --rev ." == r"""
    abort: cannot use both --rev and --restack
    [255]"""
sh % "hg rebase --restack --source ." == r"""
    abort: cannot use both --source and --restack
    [255]"""
sh % "hg rebase --restack --base ." == r"""
    abort: cannot use both --base and --restack
    [255]"""
sh % "hg rebase --restack --abort" == r"""
    abort: cannot use both --abort and --restack
    [255]"""
sh % "hg rebase --restack --continue" == r"""
    abort: cannot use both --continue and --restack
    [255]"""
sh % "hg rebase --restack --hidden" == r"""
    abort: cannot use both --hidden and --restack
    [255]"""

# Test basic case of a single amend in a small stack.
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "showgraph" == r"""
    @  4 c54ee8acf83d add b
    |
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 2:4538525df7e2 "add c"
    rebasing 3:47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    o  6 a5821d37b1b7 add d
    |
    o  5 1764a93dcfdf add c
    |
    @  4 c54ee8acf83d add b
    |
    o  0 1f0dee641bb7 add a"""

# Test multiple amends of same commit.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "hg up 1" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  2 4538525df7e2 add c
    |
    @  1 7c3bad9141dc add b
    |
    o  0 1f0dee641bb7 add a"""

sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "echo b" >> "b"
sh % "hg amend"
sh % "showgraph" == r"""
    @  4 c4c2e155d8e8 add b
    |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == 'rebasing 2:4538525df7e2 "add c"'
sh % "showgraph" == r"""
    o  5 81909827477c add c
    |
    @  4 c4c2e155d8e8 add b
    |
    o  0 1f0dee641bb7 add a"""

# Test conflict during rebasing.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "mkcommit e"
sh % "hg up 1" == "0 files updated, 0 files merged, 3 files removed, 0 files unresolved"
sh % "echo conflict" > "d"
sh % "hg add d"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "showgraph" == r"""
    @  5 f19e51f4d5ab add b
    |
    | o  4 9d206ffc875e add e
    | |
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 2:4538525df7e2 "add c"
    rebasing 3:47d2a3944de8 "add d"
    merging d
    warning: 1 conflicts while merging d! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see hg resolve, then hg rebase --continue)
    [1]"""
sh % "hg rebase --restack" == r"""
    abort: rebase in progress
    (use 'hg rebase --continue' or 'hg rebase --abort')
    [255]"""
sh % "echo merged" > "d"
sh % "hg resolve --mark d" == r"""
    (no more unresolved files)
    continue: hg rebase --continue"""
sh % "hg rebase --continue" == r'''
    already rebased 2:4538525df7e2 "add c" as 5532778357fd
    rebasing 3:47d2a3944de8 "add d"
    rebasing 4:9d206ffc875e "add e"'''
sh % "showgraph" == r"""
    o  8 661cf3d06b05 add e
    |
    o  7 fa39a0ff8b82 add d
    |
    o  6 5532778357fd add c
    |
    @  5 f19e51f4d5ab add b
    |
    o  0 1f0dee641bb7 add a"""

# Test finding a stable base commit from within the old stack.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 3" == "3 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  4 c54ee8acf83d add b
    |
    | @  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 2:4538525df7e2 "add c"
    rebasing 3:47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    @  6 a5821d37b1b7 add d
    |
    o  5 1764a93dcfdf add c
    |
    o  4 c54ee8acf83d add b
    |
    o  0 1f0dee641bb7 add a"""

# Test finding a stable base commit from a new child of the amended commit.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "mkcommit e"
sh % "showgraph" == r"""
    @  5 c1992d8998fa add e
    |
    o  4 c54ee8acf83d add b
    |
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 2:4538525df7e2 "add c"
    rebasing 3:47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    o  7 a5821d37b1b7 add d
    |
    o  6 1764a93dcfdf add c
    |
    | @  5 c1992d8998fa add e
    |/
    o  4 c54ee8acf83d add b
    |
    o  0 1f0dee641bb7 add a"""

# Test finding a stable base commit when there are multiple amends and
# a commit on top of one of the obsolete intermediate commits.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "mkcommit e"
sh % "hg prev" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    [*] add b (glob)"""
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of c54ee8acf83d are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 5" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  6 c4c2e155d8e8 add b
    |
    | @  5 c1992d8998fa add e
    | |
    | x  4 c54ee8acf83d add b
    |/
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 2:4538525df7e2 "add c"
    rebasing 3:47d2a3944de8 "add d"
    rebasing 5:c1992d8998fa "add e"'''
sh % "showgraph" == r"""
    @  9 d52a0251f17f add e
    |
    | o  8 98270af1e2d8 add d
    | |
    | o  7 81909827477c add c
    |/
    o  6 c4c2e155d8e8 add b
    |
    o  0 1f0dee641bb7 add a"""

# Test that we start from the bottom of the stack. (Previously, restack would
# only repair the unstable children closest to the current changeset. This
# behavior is now incorrect -- restack should always fix the whole stack.)
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 2" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo c" >> "c"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 3" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  5 a43fcd08f41f add c
    |
    | o  4 c54ee8acf83d add b
    | |
    | | @  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    x |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 5:a43fcd08f41f "add c" (tip)
    rebasing 3:47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    @  7 eb17ba71882a add d
    |
    o  6 036064f96b60 add c
    |
    o  4 c54ee8acf83d add b
    |
    o  0 1f0dee641bb7 add a"""

# Test what happens if there is no base commit found. The command should
# fix up everything above the current commit, leaving other commits
# below the current commit alone.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "mkcommit e"
sh % "hg up 3" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo d" >> "d"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 47d2a3944de8 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 0" == "0 files updated, 0 files merged, 3 files removed, 0 files unresolved"
sh % "mkcommit f"
sh % "hg up 1" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  6 79bfbab36011 add f
    |
    | o  5 c5b31a43cdc5 add d
    | |
    | | o  4 9d206ffc875e add e
    | | |
    | | x  3 47d2a3944de8 add d
    | |/
    | o  2 4538525df7e2 add c
    | |
    | @  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == 'rebasing 4:9d206ffc875e "add e"'
sh % "showgraph" == r"""
    o  7 2f4697538c37 add e
    |
    | o  6 79bfbab36011 add f
    | |
    o |  5 c5b31a43cdc5 add d
    | |
    o |  2 4538525df7e2 add c
    | |
    @ |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""

# Test having an unamended commit.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "hg prev" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    [*] add b (glob)"""
sh % "echo b" >> "b"
sh % "hg amend -m Amended" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "echo b" >> "b"
sh % "hg amend -m Unamended"
sh % "hg unamend"
sh % "hg up -C 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  3 a5c6a6c11eb6 Amended
    |
    | o  2 4538525df7e2 add c
    | |
    | @  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 2:4538525df7e2 "add c"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  5 34b2b8ed2689 add c
    |
    @  3 a5c6a6c11eb6 Amended
    |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""

# Revision 2 "add c" is already stable (not orphaned) so restack does nothing:

sh % "hg rebase --restack" == "nothing to rebase - empty destination"

# Test recursive restacking -- basic case.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 2" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo c" >> "c"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 1" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  5 a43fcd08f41f add c
    |
    | o  4 c54ee8acf83d add b
    | |
    | | o  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    @ |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 5:a43fcd08f41f "add c" (tip)
    rebasing 3:47d2a3944de8 "add d"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  7 eb17ba71882a add d
    |
    o  6 036064f96b60 add c
    |
    @  4 c54ee8acf83d add b
    |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""

# Test recursive restacking -- more complex case. This test is designed to
# to check for a bug encountered if rebasing is performed naively from the
# bottom-up wherein obsolescence information for commits further up the
# stack is lost upon rebasing lower levels.
sh % "newrepo"
sh % "mkcommit a"
sh % "mkcommit b"
sh % "mkcommit c"
sh % "mkcommit d"
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "echo b" >> "b"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "mkcommit e"
sh % "mkcommit f"
sh % "hg prev" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    [*] add e (glob)"""
sh % "echo e" >> "e"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of c1992d8998fa are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 2" == "2 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo c" >> "c"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "mkcommit g"
sh % "mkcommit h"
sh % "hg prev" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    [*] add g (glob)"""
sh % "echo g" >> "g"
sh % "hg amend" == r"""
    hint[amend-restack]: descendants of 0261378a5dc1 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  11 604f34a1983d add g
    |
    | o  10 9f2a7cefd4b4 add h
    | |
    | x  9 0261378a5dc1 add g
    |/
    o  8 a43fcd08f41f add c
    |
    | o  7 581ab389e415 add e
    | |
    | | o  6 2a79e3a98cd6 add f
    | | |
    | | x  5 c1992d8998fa add e
    | |/
    | o  4 c54ee8acf83d add b
    | |
    | | o  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    @ |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 6:2a79e3a98cd6 "add f"
    rebasing 8:a43fcd08f41f "add c"
    rebasing 11:604f34a1983d "add g" (tip)
    rebasing 3:47d2a3944de8 "add d"
    rebasing 10:9f2a7cefd4b4 "add h"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  16 02e6213e1c27 add h
    |
    | o  15 eb17ba71882a add d
    | |
    o |  14 cc14f12b1775 add g
    |/
    o  13 036064f96b60 add c
    |
    | o  12 d27ae4ec354c add f
    | |
    | o  7 581ab389e415 add e
    |/
    @  4 c54ee8acf83d add b
    |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
