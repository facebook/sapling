# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo a"
sh % "drawdag" << r"""
A
|
B
"""

# Create a local tag:

sh % "hg tag -l -r $A tag1"
sh % "hg tags" == r"""
    tip                                1:25c348c2bb87
    tag1                               1:25c348c2bb87"""

sh % "hg update -r tag1 -q"

# Tag does not move with commit:

sh % "hg ci -m C --config 'ui.allowemptycommit=1'"
sh % "hg log -r tag1 -T '{desc}\\n'" == "A"

# When tag and bookmark conflict, resolve bookmark first:

sh % "hg bookmark -ir $B tag1" == r"""
    bookmark tag1 matches a changeset hash
    (did you leave a -r out of an 'hg bookmark' command?)"""
sh % "hg bookmarks" == "   tag1                      0:fc2b737bb2e5"
sh % "hg log -r tag1 -T '{desc}\\n'" == "B"

sh % "hg bookmark -d tag1"
sh % "hg log -r tag1 -T '{desc}\\n'" == "A"

# Templates:

sh % "hg log -r $A -T '{tags}\\n'" == "tag1"

# Delete a tag:

sh % "hg tag -l --remove tag1"
sh % "hg tags" == "tip                                2:6a5655092097"
