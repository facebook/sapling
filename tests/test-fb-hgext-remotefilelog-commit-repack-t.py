# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". $TESTDIR/library.sh"

sh % "hginit master"
sh % "cd master"
sh % "setconfig 'remotefilelog.server=True'"
sh % "cd .."

sh % "hgcloneshallow ssh://user@dummy/master client" == r"""
    streaming all changes
    0 files to transfer, 0 bytes of data
    transferred 0 bytes in 0.0 seconds (0 bytes/sec)
    no changes found
    updating to branch default
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd client"

sh % "setconfig 'remotefilelog.commitsperrepack=1'"

sh % "echo x" > "x"
sh % "hg commit -Am x" == r"""
    adding x
    (running background incremental repack)"""
