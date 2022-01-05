# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "configure dummyssh"
sh % "setconfig experimental.allowfilepeer=True"
sh % "enable amend commitcloud infinitepush rebase remotenames share"

(
    sh % "cat"
    << r"""
[infinitepush]
branchpattern = re:scratch/.*
[commitcloud]
hostname = testhost
[experimental]
evolution = createmarkers, allowunstable
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

# Make shared part of config
(
    sh % "cat"
    << r"""
[commitcloud]
servicetype = local
servicelocation = $TESTTMP
token_enforced = False
education_page = https://someurl.com/wiki/CommitCloud
"""
    >> "shared.rc"
)

# Make a clone of the server
sh % "hg clone 'ssh://user@dummy/server' client1 -q"
sh % "cd client1"
sh % "cat ../shared.rc" >> ".hg/hgrc"

# Check generation of default workspace name based on user name and email
sh % "hg cloud join" == r"""
    commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/test/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/test/default' workspace"
sh % "'HGUSER=Test Longname <test.longname@example.com>' hg cloud join" == r"""
    commitcloud: this repository is now connected to the 'user/test.longname@example.com/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/test.longname@example.com/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/test.longname@example.com/default' workspace"
sh % "'HGUSER=Test Longname <test.longname@example.com>' hg cloud join --config 'commitcloud.email_domains=example.com'" == r"""
    commitcloud: this repository is now connected to the 'user/test.longname/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/test.longname/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/test.longname/default' workspace"
sh % "'HGUSER=Another Domain <other.longname@example.org>' hg cloud join --config 'commitcloud.email_domains=example.com'" == r"""
    commitcloud: this repository is now connected to the 'user/other.longname@example.org/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other.longname@example.org/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other.longname@example.org/default' workspace"

# Can join workspaces using raw workspace names
sh % "hg cloud join --raw-workspace project/unsupported" == r"""
    commitcloud: this repository is now connected to the 'project/unsupported' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'project/unsupported'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'project/unsupported' workspace"

# Test deprecated joining a user workspace via full workspace name
sh % "hg cloud join -w user/other/work" == r"""
    specifying full workspace names with '-w' is deprecated
    (use '-u' to select another user's workspaces)
    commitcloud: this repository is now connected to the 'user/other/work' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other/work'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other/work' workspace"

# But specifying a user and a workspace name like this just treats the workspace name as-is.
sh % "hg cloud join -u other -w user/nested/name" == r"""
    commitcloud: this repository is now connected to the 'user/other/user/nested/name' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other/user/nested/name'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other/user/nested/name' workspace"

# Test joining other users' workspaces the right way
sh % "hg cloud join -u other -w work" == r"""
    commitcloud: this repository is now connected to the 'user/other/work' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other/work'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other/work' workspace"

# Test joining other users' default workspace
sh % "hg cloud join -u other" == r"""
    commitcloud: this repository is now connected to the 'user/other/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other/default' workspace"

# Test joining other user's workspace by matching domain email
sh % "hg cloud join -u 'other@example.com' --config 'commitcloud.email_domains=example.net example.com'" == r"""
    commitcloud: this repository is now connected to the 'user/other/default' workspace for the 'server' repo
    commitcloud: synchronizing 'server' with 'user/other/default'
    commitcloud: commits synchronized
    finished in * (glob)"""
sh % "hg cloud leave" == "commitcloud: this repository is now disconnected from the 'user/other/default' workspace"
