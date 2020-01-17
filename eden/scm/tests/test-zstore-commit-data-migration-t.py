# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh.setmodernconfig()

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

# Test the revlog-fallback mode migrates draft commits.

sh.setconfig("format.use-zstore-commit-data=off")

sh % "newrepo"
sh % "drawdag" << r"""
B
|
A
""" == ""

# Make A public.
sh.hg("debugremotebookmark", "master", "desc(A)")
sh % "hg log -r 'public()' -T '{desc} '" == "A"
sh % "hg log -r 'draft()' -T '{desc} '" == "B"

# Migrate.
sh.setconfig(
    "format.use-zstore-commit-data=on",
    "format.use-zstore-commit-data-revlog-fallback=on",
)

sh % "hg log -r 'desc(A)' -T '{desc}'" == "A"
sh % "hg log -r 'desc(B)' -T '{desc}'" == "B"

# Break revlog.

sh % "mv .hg/store/00changelog.d .hg/store/00changelog.d.bak"

# "$A" can no longer be accessed because it was public.

sh % "hg log -r $A -T '{desc}'" == r"""
    abort:*00changelog.d* (glob)
    [255]"""
sh % "hg log -r $B -T '{desc}'" == "B"
