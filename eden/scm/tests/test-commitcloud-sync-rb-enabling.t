#require no-eden

  $ enable amend commitcloud infinitepush
  $ configure dummyssh
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server

  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

  $ newserver server
  $ cd $TESTTMP/server
  $ echo base > base
  $ hg commit -Aqm base
  $ hg bookmark base
  $ hg bookmark master
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk

Set remotebookmarkssync True initially for the first repo and False for the second repo

  $ cd $TESTTMP
  $ clone server client1
  $ cd client1
  $ setconfig remotenames.selectivepulldefault=master,base
  $ setconfig commitcloud.remotebookmarkssync=True
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ hg pull -q
  $ showgraph
  @  base: public  remote/base remote/master
  $ cd $TESTTMP
  $ clone server client2
  $ cd client2
  $ setconfig remotenames.selectivepulldefault=master,base
  $ setconfig commitcloud.remotebookmarkssync=False
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ hg pull -q
  $ showgraph
  @  base: public  remote/base remote/master

Advance master
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public1

Pull in client1 (remote bookmarks sync enabled)
  $ cd $TESTTMP/client1
  $ hg pull -q
  $ hg cloud sync -q
  $ showgraph
  o  public1: public  remote/master
  │
  @  base: public  remote/base

Sync in client2 (remote bookmarks sync disabled). The master bookmark doesn't move
  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ showgraph
  @  base: public  remote/base remote/master

Sync in client2 with sync enabled
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  public1: public  remote/master
  │
  @  base: public  remote/base

Sync in client1 again.
  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ showgraph
  o  public1: public  remote/master
  │
  @  base: public  remote/base

Sync in client2 again (remote bookmarks sync disabled)
  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ showgraph
  o  public1: public  remote/master
  │
  @  base: public  remote/base

Advance master
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public2

Pull in client1 and sync
  $ cd $TESTTMP/client1
  $ hg pull -q
  $ hg cloud sync -q
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public  remote/base

Sync in client 2 with remotebookmarks sync enabled.
  $ cd $TESTTMP/client2
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public  remote/base

Delete the base bookmark on the server
  $ cd $TESTTMP/server
  $ hg book -d base

Pull in client 1, which removes the base remote bookmark
  $ cd $TESTTMP/client1
  $ hg pull -q
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public

Make an update to the cloud workspace in client 2 with remotebookmarks sync disabled
  $ cd $TESTTMP/client2
  $ hg book local1
  $ hg cloud sync -q
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public local1 remote/base

Sync in client1, deleted base bookmark remains deleted
  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public local1

Sync in client2 with remote bookmarks sync enabled.  The base bookmark
gets revived in the cloud workspace as this client didn't know that it
had been deleted on the server.
  $ cd $TESTTMP/client2
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public local1 remote/base
Pull in client 2, base bookmark is now deleted
  $ hg pull
  pulling from test:server

Sync again, and this time it gets deleted.
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public local1

And remains deleted in client 1
  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ showgraph
  o  public2: public  remote/master
  │
  o  public1: public
  │
  @  base: public local1

