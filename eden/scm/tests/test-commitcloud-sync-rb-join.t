#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure modern

  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

  $ newserver server
  $ cd $TESTTMP/server
  $ echo base > base
  $ hg commit -Aqm base
  $ echo 1 > public1
  $ hg commit -Aqm public1
  $ hg bookmark master

  $ cd $TESTTMP
  $ clone server client1
  $ cd client1
  $ hg up -q remote/master
  $ hg cloud sync -q
  $ showgraph
  @  public1: public  remote/master
  │
  o  base: public
  

  $ cd $TESTTMP
  $ cd server
  $ echo 2 > public2
  $ hg commit -Aqm public2

  $ cd $TESTTMP
  $ clone server client2
  $ cd client2
  $ hg up -q remote/master
  $ hg cloud sync -q
  $ showgraph
  @  public2: public  remote/master
  │
  o  public1: public
  │
  o  base: public
  

  $ cd $TESTTMP
  $ cd client1
  $ hg cloud sync -q
  $ showgraph
  o  public2: public  remote/master
  │
  @  public1: public
  │
  o  base: public
  

  $ echo 1 > file
  $ hg commit -Aqm draft1
  $ hg cloud sync -q

  $ cd $TESTTMP
  $ cd client2
  $ hg cloud sync -q
  $ showgraph
  o  draft1: draft
  │
  │ @  public2: public  remote/master
  ├─╯
  o  public1: public
  │
  o  base: public
  
