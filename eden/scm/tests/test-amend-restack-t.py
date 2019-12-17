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


sh % "enable amend rebase remotenames"
sh % "setconfig experimental.evolution.allowdivergence=True experimental.narrow-heads=True"
sh % 'setconfig "experimental.evolution=createmarkers, allowunstable"'
sh % "setconfig visibility.enabled=true mutation.record=true mutation.enabled=true mutation.date='0 0' experimental.evolution= remotenames.rename.default=remote"
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
    @  4 743396f58c5c add b
    |
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 4538525df7e2 "add c"
    rebasing 47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    o  6 228a9d754739 add d
    |
    o  5 6d61804ea72c add c
    |
    @  4 743396f58c5c add b
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
    @  4 af408d76932d add b
    |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == 'rebasing 4538525df7e2 "add c"'
sh % "showgraph" == r"""
    o  5 e5f1b912c5fa add c
    |
    @  4 af408d76932d add b
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
    @  5 e067a66b3532 add b
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
    rebasing 4538525df7e2 "add c"
    rebasing 47d2a3944de8 "add d"
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
    already rebased 4538525df7e2 "add c" as 217450801891
    rebasing 47d2a3944de8 "add d"
    rebasing 9d206ffc875e "add e"'''
sh % "showgraph" == r"""
    o  8 b706583c96e3 add e
    |
    o  7 e247890f1a49 add d
    |
    o  6 217450801891 add c
    |
    @  5 e067a66b3532 add b
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
    o  4 743396f58c5c add b
    |
    | @  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 4538525df7e2 "add c"
    rebasing 47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    @  6 228a9d754739 add d
    |
    o  5 6d61804ea72c add c
    |
    o  4 743396f58c5c add b
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
    @  5 58e16e5d23eb add e
    |
    o  4 743396f58c5c add b
    |
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 4538525df7e2 "add c"
    rebasing 47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    o  7 228a9d754739 add d
    |
    o  6 6d61804ea72c add c
    |
    | @  5 58e16e5d23eb add e
    |/
    o  4 743396f58c5c add b
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
    hint[amend-restack]: descendants of 743396f58c5c are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 5" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  6 af408d76932d add b
    |
    | @  5 58e16e5d23eb add e
    | |
    | x  4 743396f58c5c add b
    |/
    | o  3 47d2a3944de8 add d
    | |
    | o  2 4538525df7e2 add c
    | |
    | x  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing 4538525df7e2 "add c"
    rebasing 47d2a3944de8 "add d"
    rebasing 58e16e5d23eb "add e"'''
sh % "showgraph" == r"""
    @  9 2220f78c83d8 add e
    |
    | o  8 d61d8c7f922c add d
    | |
    | o  7 e5f1b912c5fa add c
    |/
    o  6 af408d76932d add b
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
    o  5 dd2a887139a3 add c
    |
    | o  4 743396f58c5c add b
    | |
    | | @  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    x |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r'''
    rebasing dd2a887139a3 "add c"
    rebasing 47d2a3944de8 "add d"'''
sh % "showgraph" == r"""
    @  7 4e2bc7d6cfea add d
    |
    o  6 afa76d04eaa3 add c
    |
    o  4 743396f58c5c add b
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
    | o  5 f2bf14e1d387 add d
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
sh % "hg rebase --restack" == 'rebasing 9d206ffc875e "add e"'
sh % "showgraph" == r"""
    o  7 a660256c6d2a add e
    |
    | o  6 79bfbab36011 add f
    | |
    o |  5 f2bf14e1d387 add d
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
    o  3 173e12a9f067 Amended
    |
    | o  2 4538525df7e2 add c
    | |
    | @  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 4538525df7e2 "add c"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  5 b7aa69de00bb add c
    |
    @  4 4d1e27c9f82b Unamended
    |
    | x  3 173e12a9f067 Amended
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
    o  5 dd2a887139a3 add c
    |
    | o  4 743396f58c5c add b
    | |
    | | o  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    @ |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing dd2a887139a3 "add c"
    rebasing 47d2a3944de8 "add d"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  7 4e2bc7d6cfea add d
    |
    o  6 afa76d04eaa3 add c
    |
    @  4 743396f58c5c add b
    |
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
    hint[amend-restack]: descendants of 58e16e5d23eb are left behind - use 'hg restack' to rebase them
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
    hint[amend-restack]: descendants of a063c2736716 are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg up 1" == "0 files updated, 0 files merged, 2 files removed, 0 files unresolved"
sh % "showgraph" == r"""
    o  11 8282a17a7483 add g
    |
    | o  10 e86422ad5d0e add h
    | |
    | x  9 a063c2736716 add g
    |/
    o  8 dd2a887139a3 add c
    |
    | o  7 e429b2ca5d8b add e
    | |
    | | o  6 849d5cce0019 add f
    | | |
    | | x  5 58e16e5d23eb add e
    | |/
    | o  4 743396f58c5c add b
    | |
    | | o  3 47d2a3944de8 add d
    | | |
    +---x  2 4538525df7e2 add c
    | |
    @ |  1 7c3bad9141dc add b
    |/
    o  0 1f0dee641bb7 add a"""
sh % "hg rebase --restack" == r"""
    rebasing 849d5cce0019 "add f"
    rebasing dd2a887139a3 "add c"
    rebasing 8282a17a7483 "add g"
    rebasing 47d2a3944de8 "add d"
    rebasing e86422ad5d0e "add h"
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "showgraph" == r"""
    o  16 5bc29b84815f add h
    |
    | o  15 4e2bc7d6cfea add d
    | |
    o |  14 c7fc06907e30 add g
    |/
    o  13 afa76d04eaa3 add c
    |
    | o  12 6aaca8e17a00 add f
    | |
    | o  7 e429b2ca5d8b add e
    |/
    @  4 743396f58c5c add b
    |
    o  0 1f0dee641bb7 add a"""
