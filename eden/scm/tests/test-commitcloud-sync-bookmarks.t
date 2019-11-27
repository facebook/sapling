  $ enable amend commitcloud infinitepush rebase remotenames pullcreatemarkers
  $ setconfig extensions.treemanifest=!
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server

  $ hg init server
  $ cd server
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ touch base
  $ hg commit -Aqm base
  $ hg phase -p .
  $ cd ..

  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ echo data > file
  $ hg commit -Aqm "draft-commit
  > Differential Revision: https://phabricator.fb.com/D1234"
  $ hg book foo
  $ hg prev -q
  [df4f53] base
  $ hg cloud sync -q
  $ cd ..

  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 00422fad0026
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 00422fad0026
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  1: 00422fad0026 draft 'draft-commit
  |  Differential Revision: https://phabricator.fb.com/D1234' foo
  @  0: df4f53cec30a public 'base'
  
  $ cd ..

Fake land the commit
  $ cd server
  $ echo 1 > serverfile
  $ hg commit -Aqm public-commit-1
  $ echo data > file
  $ hg commit -Aqm "landed-commit
  > Differential Revision: https://phabricator.fb.com/D1234"
  $ echo 2 > serverfile
  $ hg commit -Aqm public-commit-2
  $ hg phase -p .
  $ cd ..

  $ cd client1
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  obsoleted 1 changesets
  new changesets 031d760782fb:67d363c9001e
  $ tglogp
  o  4: 67d363c9001e public 'public-commit-2'
  |
  o  3: 441f69264760 public 'landed-commit
  |  Differential Revision: https://phabricator.fb.com/D1234'
  o  2: 031d760782fb public 'public-commit-1'
  |
  | x  1: 00422fad0026 draft 'draft-commit
  |/   Differential Revision: https://phabricator.fb.com/D1234' foo
  @  0: df4f53cec30a public 'base'
  
  $ hg cloud sync -q
  $ cd ../client2
  $ hg cloud sync -q
  $ tglogp
  x  1: 00422fad0026 draft 'draft-commit
  |  Differential Revision: https://phabricator.fb.com/D1234' foo
  @  0: df4f53cec30a public 'base'
  

Rebasing the bookmark will make the draft commit disappear.

  $ cd ../client1
  $ hg rebase -b foo -d 4
  note: not rebasing 00422fad0026 "draft-commit" (foo), already in destination as 441f69264760 "landed-commit"
  $ tglogp
  o  4: 67d363c9001e public 'public-commit-2' foo
  |
  o  3: 441f69264760 public 'landed-commit
  |  Differential Revision: https://phabricator.fb.com/D1234'
  o  2: 031d760782fb public 'public-commit-1'
  |
  @  0: df4f53cec30a public 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  4: 67d363c9001e public 'public-commit-2' foo
  |
  o  3: 441f69264760 public 'landed-commit
  |  Differential Revision: https://phabricator.fb.com/D1234'
  o  2: 031d760782fb public 'public-commit-1'
  |
  @  0: df4f53cec30a public 'base'
  
Sync in client2.   This will omit the bookmark because we don't have the landed commit.

  $ cd ../client2
  $ hg cloud sync -q
  67d363c9001e1d7227625f0fa5004aca4572d214 not found, omitting foo bookmark
  $ tglogp
  @  0: df4f53cec30a public 'base'
  
Pull so that we have the public commit and sync again.

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  new changesets 031d760782fb:67d363c9001e
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

The draft commit is also gone from here, and the workspace is stable.

  $ tglogp
  o  4: 67d363c9001e public 'public-commit-2' foo
  |
  o  3: 441f69264760 public 'landed-commit
  |  Differential Revision: https://phabricator.fb.com/D1234'
  o  2: 031d760782fb public 'public-commit-1'
  |
  @  0: df4f53cec30a public 'base'
  

  $ cd ../client1
  $ hg cloud sync -q
  $ tglogp
  o  4: 67d363c9001e public 'public-commit-2' foo
  |
  o  3: 441f69264760 public 'landed-commit
  |  Differential Revision: https://phabricator.fb.com/D1234'
  o  2: 031d760782fb public 'public-commit-1'
  |
  @  0: df4f53cec30a public 'base'
  
