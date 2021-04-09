# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig \"remotefilelog.cachepath=$TESTTMP/cache\" 'extensions.remotefilelog='"

sh % "newrepo"
sh % "echo remotefilelog" >> ".hg/requires"
sh % "drawdag" << r"""
E  # E/X=1 (renamed from Y)
|
D  # D/Y=3 (renamed from X)
|
B  # B/X=2
|
A  # A/X=1
"""

sh % 'hg bundle --all "$TESTTMP/bundle" --traceback -q'

sh % "newrepo"
sh % "echo remotefilelog" >> ".hg/requires"
sh % 'hg unbundle "$TESTTMP/bundle"' == r"""
    adding changesets
    adding manifests
    adding file changes
    added 4 changesets with 8 changes to 6 files"""
