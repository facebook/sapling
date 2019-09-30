# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "'CACHEDIR=`pwd`/hgcache'"

sh % '. "$TESTDIR/library.sh"'

sh % "hg init client1"
sh % "cd client1"
sh % "cat" << r"""
[extensions]
fastmanifest=
treemanifest=

[remotefilelog]
reponame=master
cachepath=$CACHEDIR

[fastmanifest]
usetree=True
usecache=False
""" >> ".hg/hgrc"

sh % "echo a" > "a"
sh % "mkdir dir"
sh % "echo b" > "dir/b"
sh % "hg commit -Aqm 'initial commit'"

sh % "hg init ../client2"
sh % "cd ../client2"
sh % "hg pull ../client1" == r"""
    pulling from ../client1
    requesting all changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 2 changes to 2 files
    new changesets a8dee6dcff44"""
