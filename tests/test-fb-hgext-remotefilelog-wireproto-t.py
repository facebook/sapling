# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"

sh % ". '$TESTDIR/library.sh'"

sh % "hginit master"
sh % "cd master"
sh % "cat" << r"""
[remotefilelog]
server=True
""" >> ".hg/hgrc"
sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "echo y" >> "x"
sh % "hg commit -qAm y"
sh % "echo z" >> "x"
sh % "hg commit -qAm z"
sh % "hg update 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "echo w" >> "x"
sh % "hg commit -qAm w"

sh % "cd .."

# Shallow clone and activate getflogheads testing extension

sh % "hgcloneshallow 'ssh://user@dummy/master' shallow --noupdate" == r"""
    streaming all changes
    3 files to transfer, 908 bytes of data
    transferred 908 bytes in 0.0 seconds (887 KB/sec)
    searching for changes
    no changes found"""
sh % "cd shallow"

sh % "cat" << r"""
[extensions]
getflogheads=$TESTDIR/getflogheads.py
""" >> ".hg/hgrc"

# Get heads of a remotefilelog

sh % "hg getflogheads x" == r"""
    2797809ca5e9c2f307d82b1345e832f655fb99a2
    ca758b402ddc91e37e3113e1a97791b537e1b7bb"""

# Get heads of a non-existing remotefilelog

sh % "hg getflogheads y" == "EMPTY"
