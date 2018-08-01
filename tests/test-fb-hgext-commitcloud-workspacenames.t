  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend =
  > infinitepush =
  > infinitepushbackup =
  > commitcloud =
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
  #commitcloud this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized
  $ hg cloud leave
  #commitcloud this repository is now disconnected from commit cloud
  $ HGUSER="Test Longname <test.longname@example.com>" hg cloud join
  #commitcloud this repository is now connected to the 'user/test.longname@example.com/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/test.longname@example.com/default'
  #commitcloud commits synchronized
  $ hg cloud leave
  #commitcloud this repository is now disconnected from commit cloud
  $ HGUSER="Test Longname <test.longname@example.com>" hg cloud join --config commitcloud.email_domain=example.com
  #commitcloud this repository is now connected to the 'user/test.longname/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/test.longname/default'
  #commitcloud commits synchronized
  $ hg cloud leave
  #commitcloud this repository is now disconnected from commit cloud
  $ HGUSER="Another Domain <other.longname@example.org>" hg cloud join --config commitcloud.email_domain=example.com
  #commitcloud this repository is now connected to the 'user/other.longname@example.org/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/other.longname@example.org/default'
  #commitcloud commits synchronized
  $ hg cloud leave
  #commitcloud this repository is now disconnected from commit cloud
