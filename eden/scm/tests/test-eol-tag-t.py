# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["no-fsmonitor"])

# https://bz.mercurial-scm.org/2493

# Testing tagging with the EOL extension

sh % "cat" << r"""
[extensions]
eol =

[eol]
native = CRLF
""" >> "$HGRCPATH"

# setup repository

sh % "hg init repo"
sh % "cd repo"
sh % "cat" << r"""
[patterns]
** = native
""" > ".hgeol"
sh % "printf 'first\\r\\nsecond\\r\\nthird\\r\\n'" > "a.txt"
sh % "hg commit --addremove -m checkin" == r"""
    adding .hgeol
    adding a.txt"""

# Tag:

sh % "hg tag 1.0"

# Rewrite .hgtags file as it would look on a new checkout:

sh % "hg update -q null"
sh % "hg update -q"

# Touch .hgtags file again:

sh % "hg tag 2.0"

sh % "cd .."
