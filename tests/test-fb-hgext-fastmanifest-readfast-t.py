# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
fastmanifest=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

# a situation that linkrev needs to be adjusted:

sh % "echo 1" > "a"
sh % "hg commit -A a -m 1"
sh % "echo 2" > "a"
sh % "hg commit -m 2"
sh % "hg up 0 -q"
sh % "echo 2" > "a"
sh % "hg commit -m '2 again' -q"

# annotate calls "introrev", which calls "_adjustlinkrev". in this case,
# "_adjustlinkrev" will fallback to the slow path that needs to call
# manifestctx."readfast":

sh % "hg annotate a" == "2: 2"
