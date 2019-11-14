# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init a"
sh % "cd a"

sh % "hg diff inexistent1 inexistent2" == r"""
    inexistent1: * (glob)
    inexistent2: * (glob)"""

sh % "echo bar" > "foo"
sh % "hg add foo"
sh % "hg ci -m 'add foo'"

sh % "echo foobar" > "foo"
sh % "hg ci -m 'change foo'"

sh % "hg --quiet diff -r 0 -r 1" == r"""
    --- a/foo	Thu Jan 01 00:00:00 1970 +0000
    +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -bar
    +foobar"""

sh % "hg diff -r 0 -r 1" == r"""
    diff -r a99fb63adac3 -r 9b8568d3af2f foo
    --- a/foo	Thu Jan 01 00:00:00 1970 +0000
    +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -bar
    +foobar"""

sh % "hg --verbose diff -r 0 -r 1" == r"""
    diff -r a99fb63adac3 -r 9b8568d3af2f foo
    --- a/foo	Thu Jan 01 00:00:00 1970 +0000
    +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -bar
    +foobar"""

sh % "hg --debug diff -r 0 -r 1" == r"""
    diff -r a99fb63adac3f31816a22f665bc3b7a7655b30f4 -r 9b8568d3af2f1749445eef03aede868a6f39f210 foo
    --- a/foo	Thu Jan 01 00:00:00 1970 +0000
    +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
    @@ -1,1 +1,1 @@
    -bar
    +foobar"""

sh % "cd .."
