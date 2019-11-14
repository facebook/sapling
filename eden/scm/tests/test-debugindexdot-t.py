# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Just exercise debugindexdot
# Create a short file history including a merge.
sh % "hg init t"
sh % "cd t"
sh % "echo a" > "a"
sh % "hg ci -qAm t1 -d '0 0'"
sh % "echo a" >> "a"
sh % "hg ci -m t2 -d '1 0'"
sh % "hg up -qC 0"
sh % "echo b" >> "a"
sh % "hg ci -m t3 -d '2 0'"
sh % "'HGMERGE=true' hg merge -q"
sh % "hg ci -m merge -d '3 0'"

sh % "hg debugindexdot .hg/store/data/a.i" == r"""
    digraph G {
    	-1 -> 0
    	0 -> 1
    	0 -> 2
    	2 -> 3
    	1 -> 3
    }"""

sh % "cd .."
