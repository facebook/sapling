#chg-compatible
#debugruntest-compatible
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig experimental.allowfilepeer=True

Test discovery with modern setup: selectivepull, visibility.

  $ configure modern
  $ enable pushrebase

  $ newserver server

  $ clone server client1
  $ clone server client2

Push 2 branches to the server.

  $ cd client1

  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

  $ hg push -r $B --to master --create -q
  $ hg push -r $C --to other --create -q

Pull exchange should only consider 1 remote head (master, B, ignore C), but
consider all visible local heads (X, Y):

  $ cd $TESTTMP/client2
  $ drawdag << 'EOS'
  > X Y Z
  >  \|/
  >   A
  > EOS

  $ hg hide $Z -q

  $ hg pull --debug 2>&1 | grep 'remote heads'
  local heads: 3; remote heads: 1 (explicit: 1); initial common: 0

  $ hg log -G -r 'all()' -T '{desc} {remotenames}'
  o  B remote/master
  │
  │ o  Y
  ├─╯
  │ o  X
  ├─╯
  o  A
  

Push exchange should only consider heads being pushed (X), and selected remote
names (master, B, ignore C):

  $ hg push -r $X --to x --create --debug 2>&1 | grep 'local heads'
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
