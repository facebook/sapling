# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[ui]
ssh = python "$TESTDIR/dummyssh"
[extensions]
commitcloud =
infinitepush =
[commitcloud]
hostname = testhost
servicetype = local
servicelocation = $TESTTMP
""" >> "$HGRCPATH"

sh % "setconfig 'remotefilelog.reponame=server'"
sh % "hg init server"
sh % "cd server"
sh % "cat" << r"""
[infinitepush]
server = yes
indextype = disk
storetype = disk
reponame = testrepo
""" >> ".hg/hgrc"

sh % "hg clone 'ssh://user@dummy/server' client -q"
sh % "cd client"


sh % "cat" << r"""
{ "workspaces_data" : { "workspaces": [ { "name": "user/test/old", "archived": true }, { "name": "user/test/default", "archived": false }  ] } }
""" >> "$TESTTMP/userworkspacesdata"

sh % "hg cloud list" == r"""
workspaces:
        default
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud join -w <workspace name> --switch` to switch to a different workspace
"""

sh % "hg cloud list --all" == r"""
workspaces:
        old (archived)
        default
run `hg cloud sl -w <workspace name>` to view the commits
run `hg cloud join -w <workspace name> --switch` to switch to a different workspace
"""
