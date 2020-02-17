# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"

sh % "cat" << r"""
a
b
c
""" > "a"
sh % "hg ci -Am adda" == "adding a"

sh % "cat" << r"""
d
e
f
""" > "a"
sh % "hg ci -m moda"

sh % "hg diff --reverse -r0 -r1" == r"""
    diff -r 2855cdcfcbb7 -r 8e1805a3cf6e a
    --- a/a	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,3 +1,3 @@
    -d
    -e
    -f
    +a
    +b
    +c"""

sh % "cat" << r"""
g
h
""" >> "a"
sh % "hg diff --reverse --nodates" == r"""
    diff -r 2855cdcfcbb7 a
    --- a/a
    +++ b/a
    @@ -1,5 +1,3 @@
     d
     e
     f
    -g
    -h"""

# should show removed file 'a' as being added
sh % "hg revert a"
sh % "hg rm a"
sh % "hg diff --reverse --nodates a" == r"""
    diff -r 2855cdcfcbb7 a
    --- /dev/null
    +++ b/a
    @@ -0,0 +1,3 @@
    +d
    +e
    +f"""

# should show added file 'b' as being removed
sh % "echo b" >> "b"
sh % "hg add b"
sh % "hg diff --reverse --nodates b" == r"""
    diff -r 2855cdcfcbb7 b
    --- a/b
    +++ /dev/null
    @@ -1,1 +0,0 @@
    -b"""
