# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
phrevset=
""" >> "$HGRCPATH"
sh % "hg init repo"
sh % "cd repo"
sh % "echo 1" > "1"
sh % "hg add 1"
sh % "hg commit -m 'Differential Revision: http.ololo.com/D1234'"
sh % "hg up -q 0"
sh % "hg up D1234" == r"""
    phrevset.callsign is not set - doing a linear search
    This will be slow if the diff was not committed recently
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
