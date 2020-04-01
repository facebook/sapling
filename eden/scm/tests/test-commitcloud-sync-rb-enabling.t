#require py2
#chg-compatible

  $ enable amend commitcloud infinitepush remotenames
  $ configure dummyssh
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server

  $ showgraph() {
  >    hg log -G -T "{rev} {desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

  $ newserver server
  $ cd $TESTTMP/server
  $ echo base > base
  $ hg commit -Aqm base
  $ hg bookmark master
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk

  $ cd $TESTTMP
  $ clone server client1
  $ cd client1
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig remotenames.selectivepullaccessedbookmarks=True
  $ setconfig commitcloud.remotebookmarkssync=True
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ showgraph
  @  0 base: public  default/master
  
  $ cd $TESTTMP
  $ clone server client2
  $ cd client2
  $ setconfig remotenames.selectivepull=True
  $ setconfig remotenames.selectivepulldefault=master
  $ setconfig remotenames.selectivepullaccessedbookmarks=True
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ showgraph
  @  0 base: public  default/master
  

Advance master
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public1

Pull in client1 (remote bookmarks sync enabled)
  $ cd $TESTTMP/client1
  $ hg pull -q
  $ hg cloud sync -q
  $ showgraph
  o  1 public1: public  default/master
  |
  @  0 base: public
  

Sync in client2 (remote bookmarks sync disabled). The master bookmark doesn't move
  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ showgraph
  @  0 base: public  default/master
  

Sync in client2 with sync enabled
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  1 public1: public  default/master
  |
  @  0 base: public
  

Sync in client1 again.
  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ showgraph
  o  1 public1: public  default/master
  |
  @  0 base: public
  

Sync in client2 again (remote bookmarks sync disabled)
  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ showgraph
  o  1 public1: public  default/master
  |
  @  0 base: public
  

Advance master
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public2

Pull in client1 and sync
  $ cd $TESTTMP/client1
  $ hg pull -q
  $ hg cloud sync -q
  $ showgraph
  o  2 public2: public  default/master
  |
  o  1 public1: public
  |
  @  0 base: public
  

Sync in client 2 with remotebookmarks sync enabled.
  $ cd $TESTTMP/client2
  $ hg cloud sync -q --config commitcloud.remotebookmarkssync=true
  $ showgraph
  o  2 public2: public  default/master
  |
  o  1 public1: public
  |
  @  0 base: public
  


