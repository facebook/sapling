#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ setconfig 'infinitepush.branchpattern=re:(^hack/.*)'
  $ newserver server

  $ clone server client1
  $ clone server client2
  $ clone server client3
  $ clone server client4
  $ clone server client5

Push 3 branches to the server.

  $ cd client1

  $ drawdag << 'EOS'
  > B C D
  >  \|/
  >   A H
  >   |/
  >   Z
  > EOS

  $ hg push -r $Z --to release --create -q
  $ hg push -r $B --to master --create -q
  $ hg push -r $C --to other --create -q
  $ hg push -r $D --to another --create -q
  $ hg push -r $H --to hack/foo --create -q

Fetch all remote names:

  $ cd $TESTTMP/client2
  $ hg pull -B other -B master -B another -B release -B hack/foo -q

Commit (draft) on "another":

  $ drawdag << 'EOS'
  > E
  > |
  > desc(D)
  > EOS

Cleanup: another is kept because of draft E, master is kept because it is
selectivepull default:

  $ hg debugcleanremotenames
  removed 3 non-essential remote bookmarks: remote/hack/foo, remote/other, remote/release

  $ hg log -T '{desc} {remotenames} {phase}' -Gr 'all()'
  o  E  draft
  │
  o  D remote/another public
  │
  │ o  B remote/master public
  ├─╯
  o  A  public
  │
  o  Z  public
  
'hide --cleanup' does the same thing:

  $ cd $TESTTMP/client3
  $ hg pull -B other -B master -B another -B release -q
  $ enable amend
  $ hg hide --cleanup
  removed 3 non-essential remote bookmarks: remote/another, remote/other, remote/release

  $ hg log -T '{desc} {remotenames} {phase}' -Gr 'all()'
  o  B remote/master public
  │
  o  A  public
  │
  o  Z  public
  
Auto cleanup triggered by remotenames.autocleanupthreshold:

  $ cd $TESTTMP/client4
  $ hg pull -B other -B master -B another -B release -q
  $ hg log -T '{desc} {remotenames} {phase}' -Gr 'all()' --config remotenames.autocleanupthreshold=1
  attempt to clean up remote bookmarks since they exceed threshold 1
  removed 3 non-essential remote bookmarks: remote/another, remote/other, remote/release
  o  B remote/master public
  │
  o  A  public
  │
  o  Z  public
  
Resethead command to reset all heads (including draft heads):

  $ cd $TESTTMP
  $ cd client5
  $ hg pull -B other -B master -B another -B release -q
  $ drawdag << 'EOS'
  > E
  > |
  > desc(D)
  > EOS

  $ hg log -T '{desc} {remotenames} {phase}' -Gr 'all()'
  o  E  draft
  │
  o  D remote/another public
  │
  │ o  C remote/other public
  ├─╯
  │ o  B remote/master public
  ├─╯
  o  A  public
  │
  o  Z remote/release public
  
  $ hg debugresetheads

Notice that only the "master" head is left:

  $ hg log -T '{desc} {remotenames} {phase}' -Gr 'all()'
  o  B remote/master public
  │
  o  A  public
  │
  o  Z  public
  

Resethead aborts on wrong configuration:

  $ hg debugresetheads --config remotenames.selectivepulldefault=foo,bar
  abort: no remote names will be left
  (is remotenames.selectivepulldefault (remote/bar remote/foo) set correctly?)
  [255]

Resethead command removes local bookmarks too:

  $ hg bookmark -r 'desc(A)' book-A
  $ hg bookmarks
     book-A                    ac2f7407182b
  $ hg debugresetheads
  $ hg bookmarks
  no bookmarks set

