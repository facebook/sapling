# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test turning zstore-commit-data on and off

sh % "setconfig format.use-zstore-commit-data=off"

sh % "newrepo"
sh % "drawdag" << r"""
B C
|/
A
"""

# Migrate up (double-writes to zstore and 00changelog.d).

sh % "setconfig format.use-zstore-commit-data=on"
sh % 'hg log -r "$C" -T "{desc}\\n"' == "C"

# Create new commits.

sh % "drawdag" << r"""
  F
 /|
D E
| |
desc(C)
"""

# With zstore-commit-data, 00changelog.d is not used for reading commits.

sh % "mv .hg/store/00changelog.d .hg/store/00changelog.d.bak"
sh % 'hg log -GT "{desc}"' == r"""
    o    F
    |\
    | o  E
    | |
    o |  D
    |/
    o  C
    |
    | o  B
    |/
    o  A"""

# Migrate down. 00changelog.d becomes required.

sh % "setconfig format.use-zstore-commit-data=off"
sh % 'hg log -GT "{desc}"' == r"""
    abort: *00changelog.d* (glob)
    [255]"""

sh % "mv .hg/store/00changelog.d.bak .hg/store/00changelog.d"
sh % 'hg log -GT "{desc}"' == r"""
    o    F
    |\
    | o  E
    | |
    o |  D
    |/
    o  C
    |
    | o  B
    |/
    o  A"""

# Create new commits.

sh % "drawdag" << r"""
H
|
G
|
desc(B)
"""

# Migrate up (double-writes to zstore and 00changelog.d).

sh % "setconfig format.use-zstore-commit-data=on"
sh % 'hg log -r "$H" -T "{desc}\\n"' == "H"
