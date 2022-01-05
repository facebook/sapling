# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "configure dummyssh"
sh % "setconfig experimental.allowfilepeer=True"
sh % "enable commitcloud infinitepush"

(
    sh % "cat"
    << r"""
[commitcloud]
hostname = testhost
servicetype = local
servicelocation = $TESTTMP
"""
    >> "$HGRCPATH"
)

sh % "setconfig 'remotefilelog.reponame=server'"
sh % "hg init server"
sh % "cd server"
(
    sh % "cat"
    << r"""
[infinitepush]
server = yes
indextype = disk
storetype = disk
reponame = testrepo
"""
    >> ".hg/hgrc"
)

sh % "hg clone 'ssh://user@dummy/server' client -q"
sh % "cd client"


(
    sh % "cat"
    << r"""
{ "workspaces_data" : { "workspaces": [ { "name": "user/test/old", "archived": true, "version": 0 }, { "name": "user/test/default", "archived": false, "version": 0 }  ] } }
"""
    >> "$TESTTMP/workspacesdata"
)

sh % "hg cloud list" == r"""
commitcloud: searching workspaces for the 'server' repo
the following commitcloud workspaces are available:
        default
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud switch -w <workspace name>` to switch to a different workspace
run `hg cloud list --all` to list all workspaces, including deleted
"""

sh % "hg cloud list --all" == r"""
commitcloud: searching workspaces for the 'server' repo
the following commitcloud workspaces are available:
        default
        old (archived)
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud switch -w <workspace name>` to switch to a different workspace
"""

sh % "hg cloud delete -w default" == r"""
commitcloud: workspace user/test/default has been deleted
"""

sh % "hg cloud delete -w default_abc" == r"""
abort: unknown workspace: user/test/default_abc
[255]
"""

sh % "hg cloud list --all" == r"""
commitcloud: searching workspaces for the 'server' repo
the following commitcloud workspaces are available:
        old (archived)
        default (archived)
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud switch -w <workspace name>` to switch to a different workspace
"""

sh % "hg cloud list" == r"""
commitcloud: searching workspaces for the 'server' repo
no active workspaces found with the prefix user/test/
"""

sh % "hg cloud undelete -w default" == r"""
commitcloud: workspace user/test/default has been restored
"""

sh % "hg cloud list" == r"""
commitcloud: searching workspaces for the 'server' repo
the following commitcloud workspaces are available:
        default
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud switch -w <workspace name>` to switch to a different workspace
run `hg cloud list --all` to list all workspaces, including deleted
"""

sh % "hg cloud undelete -w old" == r"""
commitcloud: workspace user/test/old has been restored
"""

sh % "hg cloud list" == r"""
commitcloud: searching workspaces for the 'server' repo
the following commitcloud workspaces are available:
        default
        old
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud switch -w <workspace name>` to switch to a different workspace
"""
