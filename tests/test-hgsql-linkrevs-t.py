# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". '$TESTDIR/hgsql/library.sh'"
sh % "initdb"
sh % "setconfig 'extensions.treemanifest=!'"

# Create initial repo that can be pulled out of order

sh % "initclient client"
sh % "cd client"
sh % "touch 0"
sh % "hg commit -qAm 0"
sh % "hg up -q null"
sh % "touch 1"
sh % "hg commit -qAm 1"
sh % "hg up -q null"
sh % "touch 0"
sh % "hg commit -qAm 2"
sh % "hg up -q null"
sh % "touch 1"
sh % "hg commit -qAm 3"
sh % "hg debugindex -m" == r"""
       rev    offset  length  delta linkrev nodeid       p1           p2
         0         0      44     -1       0 a84de0447720 000000000000 000000000000
         1        44      44     -1       1 eff23848989b 000000000000 000000000000"""
sh % "cd .."

# Verify pulling out of order filelog linkrevs get reordered.
# (a normal mercurial pull here would result in order 1->0 instead of 0->1)

sh % "initserver master masterrepo"
sh % "cd master"
sh % "hg pull -q -r -2 -r -3 ../client"
sh % "hg log --template 'rev: {rev} desc: {desc}\\n'" == r"""
    rev: 1 desc: 2
    rev: 0 desc: 1"""
sh % "hg debugindex -m" == r"""
       rev    offset  length  delta linkrev nodeid       p1           p2
         0         0      44     -1       0 eff23848989b 000000000000 000000000000
         1        44      44     -1       1 a84de0447720 000000000000 000000000000"""
sh % "cd .."
