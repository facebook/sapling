# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"
sh % "hg debugdrawdag" << r"""
J K
|/|
H I
| |
F G
|/
E
|\
A D
|\|
B C
"""

sh % "hg debugbindag -r '::A' -o a.dag"
sh % "hg debugpreviewbindag a.dag" == r"""
    o    2
    |\
    o |  1
     /
    o  0"""


sh % "hg debugbindag -r '::J' -o j.dag"
sh % "hg debugpreviewbindag j.dag" == r"""
    o  7
    |
    o  6
    |
    o  5
    |
    o    4
    |\
    | o  3
    | |
    o |  2
    |\|
    | o  1
    |
    o  0"""

sh % "hg debugbindag -r 'all()' -o all.dag"
sh % "hg debugpreviewbindag all.dag" == r"""
    o    10
    |\
    | | o  9
    | |/
    o |  8
    | |
    | o  7
    | |
    o |  6
    | |
    | o  5
    |/
    o    4
    |\
    | o  3
    | |
    o |  2
    |\|
    | o  1
    |
    o  0"""
