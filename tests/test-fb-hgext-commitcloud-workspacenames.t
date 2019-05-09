  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > infinitepushbackup =
  > rebase =
  > remotenames =
  > share =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [commitcloud]
  > hostname = testhost
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > auth_help = visit https://localhost/oauth to generate a registration token
  > education_page = https://someurl.com/wiki/CommitCloud
  > owner_team = The Test Team @ FB
  > EOF

Make a clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful

Check generation of default workspace name based on user name and email
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud
  $ HGUSER="Test Longname <test.longname@example.com>" hg cloud join
  commitcloud: this repository is now connected to the 'user/test.longname@example.com/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test.longname@example.com/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud
  $ HGUSER="Test Longname <test.longname@example.com>" hg cloud join --config commitcloud.email_domain=example.com
  commitcloud: this repository is now connected to the 'user/test.longname/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test.longname/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud
  $ HGUSER="Another Domain <other.longname@example.org>" hg cloud join --config commitcloud.email_domain=example.com
  commitcloud: this repository is now connected to the 'user/other.longname@example.org/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other.longname@example.org/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

Can join workspaces using raw workspace names
  $ hg cloud join --raw-workspace project/unsupported
  commitcloud: this repository is now connected to the 'project/unsupported' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'project/unsupported'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

Test deprecated joining a user workspace via full workspace name
  $ hg cloud join -w user/other/work
  specifying full workspace names with '-w' is deprecated
  (use '-u' to select another user's workspaces)
  commitcloud: this repository is now connected to the 'user/other/work' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other/work'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

But specifying a user and a workspace name like this just treats the workspace name as-is.
  $ hg cloud join -u other -w user/nested/name
  commitcloud: this repository is now connected to the 'user/other/user/nested/name' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other/user/nested/name'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

Test joining other users' workspaces the right way
  $ hg cloud join -u other -w work
  commitcloud: this repository is now connected to the 'user/other/work' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other/work'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

Test joining other users' default workspace
  $ hg cloud join -u other
  commitcloud: this repository is now connected to the 'user/other/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud

Test joining other user's workspace by matching domain email
  $ hg cloud join -u other@example.com --config commitcloud.email_domain=example.com
  commitcloud: this repository is now connected to the 'user/other/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/other/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud
