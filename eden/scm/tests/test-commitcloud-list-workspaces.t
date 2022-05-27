#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ configure dummyssh
  $ setconfig experimental.allowfilepeer=True
  $ enable commitcloud infinitepush

  $ cat >> $HGRCPATH << EOF
  > [commitcloud]
  > hostname = testhost
  > servicetype = local
  > servicelocation = $TESTTMP
  > EOF

  $ setconfig 'remotefilelog.reponame=server'
  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << 'EOF'
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

  $ hg clone 'ssh://user@dummy/server' client -q
  $ cd client

  $ cat >> $TESTTMP/workspacesdata << 'EOF'
  > { "workspaces_data" : { "workspaces": [ { "name": "user/test/old", "archived": true, "version": 0 }, { "name": "user/test/default", "archived": false, "version": 0 }  ] } }
  > EOF

  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace
  run `hg cloud list --all` to list all workspaces, including deleted


  $ hg cloud list --all
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
          old (archived)
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace


  $ hg cloud delete -w default
  commitcloud: workspace user/test/default has been deleted


  $ hg cloud delete -w default_abc
  abort: unknown workspace: user/test/default_abc
  [255]


  $ hg cloud list --all
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          old (archived)
          default (archived)
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace


  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  no active workspaces found with the prefix user/test/


  $ hg cloud undelete -w default
  commitcloud: workspace user/test/default has been restored


  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace
  run `hg cloud list --all` to list all workspaces, including deleted


  $ hg cloud undelete -w old
  commitcloud: workspace user/test/old has been restored


  $ hg cloud list
  commitcloud: searching workspaces for the 'server' repo
  the following commitcloud workspaces are available:
          default
          old
  run `hg cloud sl -w <workspace name>` to view the commits
  run `hg cloud switch -w <workspace name>` to switch to a different workspace

