# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"

sh % "mkdir alpha"
sh % "touch alpha/one"
sh % "mkdir beta"
sh % "touch beta/two"

sh % "hg add alpha/one beta/two"
sh % "hg ci -m start"

sh % "echo 1" > "alpha/one"
sh % "echo 2" > "beta/two"

# everything

sh % "hg diff --nodates" == r"""
    diff -r * alpha/one (glob)
    --- a/alpha/one
    +++ b/alpha/one
    @@ -0,0 +1,1 @@
    +1
    diff -r * beta/two (glob)
    --- a/beta/two
    +++ b/beta/two
    @@ -0,0 +1,1 @@
    +2"""

# beta only

sh % "hg diff --nodates beta" == r"""
    diff -r * beta/two (glob)
    --- a/beta/two
    +++ b/beta/two
    @@ -0,0 +1,1 @@
    +2"""

# inside beta

sh % "cd beta"
sh % "hg diff --nodates ." == r"""
    diff -r * beta/two (glob)
    --- a/beta/two
    +++ b/beta/two
    @@ -0,0 +1,1 @@
    +2"""

# relative to beta

sh % "cd .."
sh % "hg diff --nodates --root beta" == r"""
    diff -r * two (glob)
    --- a/two
    +++ b/two
    @@ -0,0 +1,1 @@
    +2"""

# inside beta

sh % "cd beta"
sh % "hg diff --nodates --root ." == r"""
    diff -r * two (glob)
    --- a/two
    +++ b/two
    @@ -0,0 +1,1 @@
    +2"""

sh % "cd .."
