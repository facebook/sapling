# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable morestatus"
sh % "setconfig morestatus.show=True ui.origbackuppath=.hg/origs"


def createstate():
    """Create an interrupted state resolving 'hg update --merge' conflicts"""

    sh % "newrepo"
    sh % "drawdag" << r"""
    B C
    |/   # B/A=B\n
    A
    """

    sh % 'hg up -C "$C" -q'
    sh % "echo C" > "A"
    sh % 'hg up --merge "$B" -q' == r"""
        warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
        [1]"""


createstate()

# There is only one working parent (which is good):
sh % 'hg parents -T "{desc}\\n"' == "B"

# 'morestatus' message:
sh % "hg status" == r"""
    M A

    # The repository is in an unfinished *merge* state.
    # Unresolved merge conflicts:
    #  (trailing space)
    #     A
    #  (trailing space)
    # To mark files as resolved:  hg resolve --mark FILE"""

# 'morestatus' message after resolve
# BAD: The unfinished merge state is confusing and there is no clear way to get out.
sh % "hg resolve -m A" == "(no more unresolved files)"
sh % "hg status" == r"""
    M A

    # The repository is in an unfinished *merge* state.
    # No unresolved merge conflicts."""

# To get rid of the state
# BAD: No clean way to get rid of it without losing changes.
