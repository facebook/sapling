# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"
sh % "drawdag" << r"""
C   # C/x/3=3
| D # C/x/2=2
|/  # D/x/4=4
B
|
A   # A/x/1=1
"""

sh % "hg update -q '$C'"

# Log a directory:

sh % "hg log -T '{desc}\\n' -f x" == r"""
    C
    A"""

# From non-repo root:

sh % "cd x"
sh % "hg log -G -T '{desc}\\n' -f ." == r"""
    @  C
    :
    o  A"""

# Using the follow revset, which is related to repo root:

sh % "hg log -G -T '{desc}\\n' -r 'follow(\"x\")'" == r"""
    @  C
    :
    o  A"""
sh % "hg log -G -T '{desc}\\n' -r 'follow(\".\")'" == r"""
    @  C
    |
    o  B
    |
    o  A"""
sh % "hg log -G -T '{desc}\\n' -r 'follow(\"relpath:.\")'" == r"""
    @  C
    :
    o  A"""
