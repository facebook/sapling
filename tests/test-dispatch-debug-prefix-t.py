# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"
sh % "hg d" == ""

sh % "hg di --config alias.did=root" == r"""
    hg: command 'di' is ambiguous:
     did
     diff
    [255]"""

sh % "hg debugf" == r"""
    hg: command 'debugf' is ambiguous:
    	debugfilerevision
    	debugfileset
    	debugformat
    	debugfsinfo
    [255]"""
