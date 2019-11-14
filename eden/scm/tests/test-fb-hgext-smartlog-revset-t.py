# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"
sh % "enable smartlog"
sh % "drawdag" << r"""
B C  # B has date 100000 0
|/   # C has date 200000 0
A
"""
sh % 'hg bookmark -ir "$A" master'
sh % "hg log -r 'smartlog()' -T '{desc}\\n'" == r"""
    A
    B
    C"""
sh % "hg log -r \"smartlog($B)\" -T '{desc}\\n'" == r"""
    A
    B"""
sh % "hg log -r \"smartlog(heads=$C, master=$B)\" -T '{desc}\\n'" == r"""
    A
    B
    C"""
sh % "hg log -r \"smartlog(master=($A::)-$B-$C)\" -T '{desc}\\n'" == r"""
    A
    B
    C"""
