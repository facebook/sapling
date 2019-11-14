# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# https://bz.mercurial-scm.org/1089

sh % "hg init"
sh % "mkdir a"
sh % "echo a" > "a/b"
sh % "hg ci -Am m" == "adding a/b"

sh % "hg rm a" == "removing a/b"
sh % "hg ci -m m a"

sh % "mkdir a b"
sh % "echo a" > "a/b"
sh % "hg ci -Am m" == "adding a/b"

sh % "hg rm a" == "removing a/b"
sh % "cd b"

# Relative delete:

sh % "hg ci -m m ../a"

sh % "cd .."
