# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable morestatus"
sh % "setconfig morestatus.show=True ui.origbackuppath=.hg/origs"


def createstate(command="update"):
    """Create an interrupted state resolving 'hg update --merge' conflicts"""

    sh % "newrepo"
    sh % "drawdag" << r"""
    B C
    |/   # B/A=B\n
    A
    """

    if command == "update":
        sh % 'hg up -C "$C" -q'
        sh % "echo C" > "A"

        sh % 'hg up --merge "$B" -q' == r"""
            warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
            [1]"""
    elif command == "backout":
        sh % "drawdag" << r"""
        D   # D/A=D\n
        |
        desc(B)
        """
        sh % 'hg up -C "$D" -q'
        sh % 'hg backout "$B" -q' == r"""
            warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
            [1]"""


createstate()

# There is only one working parent (which is good):
sh % 'hg parents -T "{desc}\\n"' == "B"

# 'morestatus' message:
sh % "hg status" == r"""
    M A

    # The repository is in an unfinished *update* state.
    # Unresolved merge conflicts:
    #  (trailing space)
    #     A
    #  (trailing space)
    # To mark files as resolved:  hg resolve --mark FILE
    # To continue:                hg update --continue
    # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)"""

# Cannot --continue right now
sh % "hg update --continue" == r"""
    abort: outstanding merge conflicts
    (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
    [255]"""

# 'morestatus' message after resolve
# BAD: The unfinished merge state is confusing and there is no clear way to get out.
sh % "hg resolve -m A" == r"""
    (no more unresolved files)
    continue: hg update --continue"""
sh % "hg status" == r"""
    M A

    # The repository is in an unfinished *update* state.
    # No unresolved merge conflicts.
    # To continue:                hg update --continue
    # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)"""

# To get rid of the state
sh % "hg update --continue"
sh % "hg status" == "M A"

# Test abort flow
createstate()
sh % "hg update --clean . -q"
sh % "hg status"

# Test 'hg continue'
sh % "hg continue" == r"""
    abort: nothing to continue
    [255]"""

createstate()
sh % "hg continue" == r"""
    abort: outstanding merge conflicts
    (use 'hg resolve --list' to list, 'hg resolve --mark FILE' to mark resolved)
    [255]"""

sh % "hg resolve -m A" == r"""
    (no more unresolved files)
    continue: hg update --continue"""

sh % "hg continue"


# Test 'hg continue' in a context that does not implement --continue.
# Choose 'backout' for this test. The 'backout' command does not have
# --continue.

createstate(command="backout")
sh % "hg continue" == r"""
    abort: outstanding merge conflicts
    (use 'hg resolve -l' to see a list of conflicted files, 'hg resolve -m' to mark files as resolved)
    [255]"""
sh % "hg resolve --all -t :local" == "(no more unresolved files)"
sh % "hg status" == r"""
    R B

    # The repository is in an unfinished *merge* state.
    # No unresolved merge conflicts."""

# The state is confusing, but 'hg continue' can resolve it.
sh % "hg continue" == "(exiting merge state)"
sh % "hg status" == "R B"
