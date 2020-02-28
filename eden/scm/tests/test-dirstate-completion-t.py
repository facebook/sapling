# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


for testcase in ["v0", "v1", "v2"]:

    sh % "cd $TESTTMP"

    if feature.check(["v0"]):
        sh % "setconfig 'format.dirstate=0'"

    if feature.check(["v1"]):
        sh % "setconfig 'format.dirstate=1'"

    if feature.check(["v2"]):
        sh % "setconfig 'format.dirstate=2'"

    sh % "newrepo"
    sh % "echo file1" > "file1"
    sh % "echo file2" > "file2"
    sh % "mkdir -p dira dirb"
    sh % "echo file3" > "dira/file3"
    sh % "echo file4" > "dirb/file4"
    sh % "echo file5" > "dirb/file5"
    sh % "hg ci -q -Am base"

    # Test debugpathcomplete with just normal files

    sh % "hg debugpathcomplete f" == r"""
        file1
        file2"""
    sh % "hg debugpathcomplete -f d" == r"""
        dira/file3
        dirb/file4
        dirb/file5"""

    # Test debugpathcomplete with removed files

    sh % "hg rm dirb/file5"
    sh % "hg debugpathcomplete -r d" == "dirb"
    sh % "hg debugpathcomplete -fr d" == "dirb/file5"
    sh % "hg rm dirb/file4"
    sh % "hg debugpathcomplete -n d" == "dira"

    # Test debugpathcomplete with merges

    sh % "cd .."
    sh % "newrepo"
    sh % "drawdag" << r"""
      D     # A/filenormal = 1
     / \    # B/filep1 = 1
    B   C   # B/filemerged = 1
     \ /    # C/filep2 = 1
      A     # C/filemerged = 2
            # D/filemerged = 12
    """
    sh % "hg up -q $D"
    sh % "hg debugpathcomplete f" == r"""
        filemerged
        filenormal
        filep1
        filep2"""
