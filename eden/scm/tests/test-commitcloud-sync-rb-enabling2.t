#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure modern

  $ newserver server
  $ cd $TESTTMP/server
  $ echo base > base
  $ hg commit -Aqm base
  $ hg bookmark master

  $ clone server client1
  $ hg --cwd client1 log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  @  base public remote/master
  

  $ clone server client2
  $ hg --cwd client2 log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  @  base public remote/master
  

Advance master

  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public1

Pull in client1

  $ cd $TESTTMP/client1
  $ hg pull -q
  $ drawdag << 'EOS'
  > X
  > |
  > desc(base)
  > EOS
  $ hg cloud sync -q
  $ hg log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  o  X draft
  │
  │ o  public1 public remote/master
  ├─╯
  @  base public
  

Advance master again.
  $ cd $TESTTMP/server
  $ echo more >> base
  $ hg commit -Aqm public2

Sync in client2. The master bookmark gets synced to the same location as in
client1, but not in the server.

  $ cd $TESTTMP/client2
  $ hg cloud sync -q
  $ hg log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  o  X draft
  │
  │ o  public1 public remote/master
  ├─╯
  @  base public
  

Make changes in client2 and sync the changes to cloud.

  $ drawdag << 'EOS'
  > Y
  > |
  > desc(X)
  > EOS
  $ hg cloud sync -q

Sync back to client1. This does not cause lagged default/master.

  $ cd $TESTTMP/client1
  $ hg cloud sync -q
  $ hg log -Gr 'all()' -T '{desc} {phase} {remotebookmarks}'
  o  Y draft
  │
  o  X draft
  │
  │ o  public1 public remote/master
  ├─╯
  @  base public
  
