# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test case that makes use of the weakness of patience diff algorithm

sh % "hg init"
open("a", "w").write("\n".join(list("a" + "x" * 10 + "u" + "x" * 30 + "a\n")))
sh % "hg commit -m 1 -A a"
open("a", "w").write("\n".join(list("b" + "x" * 30 + "u" + "x" * 10 + "b\n")))
sh % "hg diff" == r"""
    diff -r f0aeecb49805 a
    --- a/a	Thu Jan 01 00:00:00 1970 +0000
    +++ b/a	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,4 +1,4 @@
    -a
    +b
     x
     x
     x
    @@ -9,7 +9,6 @@
     x
     x
     x
    -u
     x
     x
     x
    @@ -30,6 +29,7 @@
     x
     x
     x
    +u
     x
     x
     x
    @@ -40,5 +40,5 @@
     x
     x
     x
    -a
    +b"""
