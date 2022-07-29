#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable amend commitcloud infinitepush rebase remotenames pullcreatemarkers phabstatus
  $ configure dummyssh
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server
  $ setconfig pullcreatemarkers.use-graphql=false
  $ setconfig pullcreatemarkers.hook-pull=true
  $ setconfig extensions.arcconfig="$TESTDIR/../edenscm/hgext/extlib/phabricator/arcconfig.py"
  $ setconfig devel.segmented-changelog-rev-compat=true

  $ hg init server --config extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ cd server
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ setconfig treemanifest.server=True extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ touch base
  $ hg commit -Aqm base
  $ hg bookmark master
  $ hg debugmakepublic .
  $ cd ..
Configure arc
  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

Client 1
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
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
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 00422fad0026 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  00422fad0026 draft 'draft-commit
  │  Differential Revision: https://phabricator.fb.com/D1234' foo
  @  df4f53cec30a public 'base'
  
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
  $ hg debugmakepublic .
  $ cat > $TESTTMP/mockduit << EOF
  > [{
  >   "data": {
  >     "phabricator_diff_query": [
  >       {
  >         "results": {
  >           "nodes": [
  >             {
  >               "number": 1234,
  >               "phabricator_versions": {
  >                 "nodes": [
  >                   {"local_commits": []}
  >                 ]
  >               },
  >               "phabricator_diff_commit": {
  >                 "nodes": [
  >                   {"commit_identifier": "441f69264760bc9126522b19ffd4ad350cb79a29"}
  >                 ]
  >               }
  >             }
  >           ]
  >         }
  >       }
  >     ]
  >   },
  >   "extensions": {
  >     "is_final": true
  >   }
  > }]
  > EOF
  $ cd ..

  $ cd client1
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  marked 1 commit as landed
  $ tglogp
  o  67d363c9001e public 'public-commit-2'
  │
  o  441f69264760 public 'landed-commit
  │  Differential Revision: https://phabricator.fb.com/D1234'
  o  031d760782fb public 'public-commit-1'
  │
  │ x  00422fad0026 draft 'draft-commit
  ├─╯  Differential Revision: https://phabricator.fb.com/D1234' foo
  @  df4f53cec30a public 'base'
  
  $ hg cloud sync -q
  $ cd ../client2
  $ hg cloud sync -q
  $ tglogp
  o  00422fad0026 draft 'draft-commit
  │  Differential Revision: https://phabricator.fb.com/D1234' foo
  @  df4f53cec30a public 'base'
  

Rebasing the bookmark will make the draft commit disappear.

  $ cd ../client1
  $ hg rebase -b foo -d 67d363c9001e1d7227625f0fa5004aca4572d214
  note: not rebasing 00422fad0026 "draft-commit" (foo), already in destination as 441f69264760 "landed-commit"
  $ tglogp
  o  67d363c9001e public 'public-commit-2' foo
  │
  o  441f69264760 public 'landed-commit
  │  Differential Revision: https://phabricator.fb.com/D1234'
  o  031d760782fb public 'public-commit-1'
  │
  @  df4f53cec30a public 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  67d363c9001e public 'public-commit-2' foo
  │
  o  441f69264760 public 'landed-commit
  │  Differential Revision: https://phabricator.fb.com/D1234'
  o  031d760782fb public 'public-commit-1'
  │
  @  df4f53cec30a public 'base'
  
Sync in client2.   This will omit the bookmark because we don't have the landed commit.

  $ cd ../client2
  $ hg cloud sync -q
  67d363c9001e not found, omitting foo bookmark
  $ tglogp
  @  df4f53cec30a public 'base'
  
Pull so that we have the public commit and sync again.

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

The draft commit is also gone from here, and the workspace is stable.

  $ tglogp
  o  67d363c9001e public 'public-commit-2' foo
  │
  o  441f69264760 public 'landed-commit
  │  Differential Revision: https://phabricator.fb.com/D1234'
  o  031d760782fb public 'public-commit-1'
  │
  @  df4f53cec30a public 'base'
  

  $ cd ../client1
  $ hg cloud sync -q
  $ tglogp
  o  67d363c9001e public 'public-commit-2' foo
  │
  o  441f69264760 public 'landed-commit
  │  Differential Revision: https://phabricator.fb.com/D1234'
  o  031d760782fb public 'public-commit-1'
  │
  @  df4f53cec30a public 'base'
  
