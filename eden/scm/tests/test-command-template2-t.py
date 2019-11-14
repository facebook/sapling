# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"

# Test ifgt function

sh % 'hg log -T \'{ifgt(2, 1, "GT", "NOTGT")} {ifgt(2, 2, "GT", "NOTGT")} {ifgt(2, 3, "GT", "NOTGT")}\\n\' -r null' == "GT NOTGT NOTGT"

sh % 'hg log -T \'{ifgt("2", "1", "GT", "NOTGT")} {ifgt("2", "2", "GT", "NOTGT")} {ifgt("2", 3, "GT", "NOTGT")}\\n\' -r null' == "GT NOTGT NOTGT"
