# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["py2"])


sh % "newrepo server"
sh % "drawdag" << r"""
  D
  |
B C
|/
A
"""
sh % 'hg bookmark -r "desc(B)" master'

# Remote bookmarks should be written even if remotenames is disabled.

sh % "newrepo client"
sh % 'setconfig "paths.default=$TESTTMP/server" "extensions.remotenames=!"'
sh % "hg pull" == r"""
    pulling from $TESTTMP/server
    requesting all changes
    adding changesets
    adding manifests
    adding file changes
    added 4 changesets with 4 changes to 4 files
    adding remote bookmark master"""
sh % 'hg dbsh -c "ui.write(repo.svfs.tryread(\\"remotenames\\") + \\"\\n\\")"' == "112478962961147124edd43549aedd1a335e44bf bookmarks default/master"

# pull -r should also pull master.

sh % "newrepo client2"
sh % 'setconfig "paths.default=$TESTTMP/server" "extensions.remotenames=!"'

sh % "hg pull -r $C" == r"""
    pulling from $TESTTMP/server
    adding changesets
    adding manifests
    adding file changes
    added 3 changesets with 3 changes to 3 files
    adding remote bookmark master"""

sh % "hg log -Gr 'all()' -T '{desc} {remotenames}'" == r"""
    o  C
    |
    | o  B default/master
    |/
    o  A"""
