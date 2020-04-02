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
  $ drawdag << 'EOS'
  > X
  > |
  > desc(base)
  > EOS
  $ hg cloud sync -q
  $ showgraph
  o  2 X: draft
  |
  | o  1 public1: public  default/master
  |/
  @  0 base: public
  
Advance master again.
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public2

Sync in client2 (remote bookmarks sync disabled). The master bookmark does not
move but the real public master got pulled by selective pull.
Without narrow-heads those public commits cannot be hidden by just visibleheads.

  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ showgraph
  o  3 X: draft
  |
  | o  2 public2: public
  | |
  | o  1 public1: public
  |/
  @  0 base: public  default/master
  

Make changes in client2 and sync the changes to cloud.

  $ drawdag << 'EOS'
  > Y
  > |
  > desc(X)
  > EOS
  $ hg cloud sync -q

Sync back to client1. This caused lagged default/master.

  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ showgraph
  o  4 Y: draft
  |
  | o  3 public2: public
  | |
  o |  2 X: draft
  | |
  | o  1 public1: public  default/master
  |/
  @  0 base: public
  
