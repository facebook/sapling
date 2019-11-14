# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh


feature.require(["no-icasefs"])

sh % "newrepo"
sh % "mkdir -p dirA/SUBDIRA dirA/subdirA dirB dirA/mixed DIRB"
sh % "touch dirA/SUBDIRA/file1 dirA/subdirA/file2 dirA/mixed/file3 dirA/Mixed dirA/MIXED dirB/file4 dirB/FILE4 DIRB/File4"
sh % "hg commit -Aqm base"

# Check for all collisions
sh % "hg debugexistingcasecollisions" == r"""
    <root> contains collisions: DIRB, dirB
    "dirA" contains collisions: MIXED, Mixed, mixed
    "dirA" contains collisions: SUBDIRA, subdirA
    "dirB" contains collisions: FILE4, file4"""

# Check for collisions in a directory
sh % "hg debugexistingcasecollisions dirB" == '"dirB" contains collisions: FILE4, file4'
