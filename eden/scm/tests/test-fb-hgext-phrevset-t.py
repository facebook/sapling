# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
phrevset=

[paths]
default=dummy://dummy
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
    abort: phrevset.graphqlonly is set and Phabricator cannot resolve D1234
    [255]"""

sh % "drawdag" << "A"
sh % "setconfig phrevset.mock-D1234=$A phrevset.callsign=R"
sh % "hg log -r D1234 -T '{desc}\n'" == "A"

# Phabricator provides an unknown commit hash.
sh % "setconfig phrevset.mock-D1234=6008bb23d775556ff6c3528541ca5a2177b4bb92"
sh % "hg log -r D1234 -T '{desc}\n'" == r"""
    pulling D1234 (6008bb23d775556ff6c3528541ca5a2177b4bb92) from 'dummy://dummy/'
    pull failed: repository dummy://dummy/ not found
    abort: unknown revision 'D1234'!
    [255]"""

# 'pull -r Dxxx' will be rewritten to 'pull -r HASH'
sh % "hg pull -r D1234 --config paths.default=." == r"""
    pulling from $TESTTMP/repo
    rewriting pull rev 'D1234' into '6008bb23d775556ff6c3528541ca5a2177b4bb92'
    abort: unknown revision '6008bb23d775556ff6c3528541ca5a2177b4bb92'!
    [255]"""
