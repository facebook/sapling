# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % '. "$TESTDIR/library.sh"'

sh % "hginit master"

sh % "cat" << r"""
[extensions]
treemanifest=
""" >> "$HGRCPATH"

sh % "cd master"
sh % "cat" << r"""
[extensions]
[remotefilelog]
server=True
[treemanifest]
server=True
""" >> ".hg/hgrc"
sh % "mkdir dir"
sh % "echo x" > "dir/x"
sh % "hg commit -qAm x1"
sh % "cd .."

# Clone with shallowtrees not set (False)

sh % "hgcloneshallow 'ssh://user@dummy/master' shallow --noupdate" == r"""
    streaming all changes
    3 files to transfer, 235 bytes of data
    transferred 235 bytes in 0.0 seconds (229 KB/sec)
    searching for changes
    no changes found"""
sh % "ls 'shallow/.hg/store/00*.i'" == r"""
    shallow/.hg/store/00changelog.i
    shallow/.hg/store/00manifesttree.i"""
sh % "rm -rf shallow"

# Clone with shallowtrees=True
sh % "cat" << r"""
[remotefilelog]
shallowtrees=True
""" >> "master/.hg/hgrc"

sh % "hgcloneshallow 'ssh://user@dummy/master' shallow --noupdate" == r"""
    streaming all changes
    2 files to transfer, 124 bytes of data
    transferred 124 bytes in 0.0 seconds (121 KB/sec)
    searching for changes
    no changes found"""
sh % "ls 'shallow/.hg/store/00*.i'" == "shallow/.hg/store/00changelog.i"
