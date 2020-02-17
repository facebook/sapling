# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test that journal and lfs wrap the share extension properly

sh % "cat" << r"""
[extensions]
journal=
lfs=
[lfs]
threshold=1000B
usercache=$TESTTMP/lfs-cache
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "echo s" > "smallfile"
sh % "hg commit -Aqm 'add small file'"
sh % "cd .."

sh % "hg --config 'extensions.share=' share repo sharedrepo" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
