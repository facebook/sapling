# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Set up
(
    sh % "cat"
    << r"""
[experimental]
evolution=all
[extensions]
amend=
tweakdefaults=
"""
    >> "$HGRCPATH"
)

# Test hg bookmark works with hidden commits

sh % "hg init repo1"
sh % "cd repo1"
sh % "touch a"
sh % "hg commit -A a -m a"
sh % "echo 1" >> "a"
sh % "hg commit a -m a1"
sh % "hg hide da7a5140a611 -q"
sh % "hg bookmark b -r da7a5140a611 -q"

# Same test but with remotenames enabled

sh % "hg bookmark b2 -r da7a5140a611 -q --config 'extensions.remotenames='"
