# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"

# Turn manifest verification on and off:
sh % "hg init repo1"
sh % "cd repo1"
sh % "hg debugdrawdag" << r"""
b c
|/
a
"""
sh % "hg verify --config 'verify.skipmanifests=0'" == r"""
    checking changesets
    checking manifests
    crosschecking files in changesets and manifests
    checking files
    3 files, 3 changesets, 3 total revisions"""
sh % "echo '[verify]'" >> "$HGRCPATH"
sh % "echo 'skipmanifests=1'" >> "$HGRCPATH"
sh % "hg verify" == r"""
    checking changesets
    verify.skipmanifests is enabled; skipping verification of manifests
    0 files, 3 changesets, 0 total revisions"""
