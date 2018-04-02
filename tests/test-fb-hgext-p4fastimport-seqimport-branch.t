#require p4

  $ . $TESTDIR/p4setup.sh

Configure clients (main and release/branch)
  $ p4 client -o | sed 's,//depot,//depot/Main,g' >p4client
  $ p4 client -i <p4client
  Client hg-p4-import saved.
  $ p4 client -o | sed "s,//depot/Main,//depot/Release,g;s,$P4CLIENT,hg-p4-branch,g" >p4client-branch
  $ p4 client -i <p4client-branch
  Client hg-p4-branch saved.

Populate depot
  $ for f in A B C; do
  > echo $f > $f
  > p4 -q add $f
  > p4 -q submit -d $f
  > done

Branch!
  $ p4 populate //depot/Main/... //depot/Release/...
  3 files branched (change 4).

Change A on Main
  $ p4 edit A; echo AA >> A
  //depot/Main/A#1 - opened for edit
  $ p4 submit -d AA
  Submitting change 5.
  Locking 1 files ...
  edit //depot/Main/A#2
  Change 5 submitted.

Change A on Release
  $ p4 -q -c hg-p4-branch sync
  $ p4 -c hg-p4-branch edit A; echo D >> A
  //depot/Release/A#1 - opened for edit
  $ p4 -q -c hg-p4-branch submit -d D

Confirm we have A,B,C on Main and Release, and A was edited on both
  $ p4 files //depot/...
  //depot/Main/A#2 - edit change 5 (text)
  //depot/Main/B#1 - add change 2 (text)
  //depot/Main/C#1 - add change 3 (text)
  //depot/Release/A#2 - edit change 6 (text)
  //depot/Release/B#1 - branch change 4 (text)
  //depot/Release/C#1 - branch change 4 (text)

A differs between Main and Release
  $ p4 print //depot/Main/A
  //depot/Main/A#2 - edit change 5 (text)
  A
  AA
  $ p4 print //depot/Release/A
  //depot/Release/A#2 - edit change 6 (text)
  A
  D

Setup hg repo
  $ cd $hgwd
  $ hg init

Import Main!!
  $ hg p4seqimport -B master -P $P4ROOT hg-p4-import

Import Release branch!!
  $ hg p4seqimport --base 2 -B release -P $P4ROOT hg-p4-branch

Confirm commit graph looks good
  $ hg log -G -T 'CL{extras.p4changelist}: {desc} ({bookmarks})\n\n'
  o  CL6: D (release)
  |
  o  CL4: Populate //depot/Main/... //depot/Release/.... ()
  |
  | o  CL5: AA (master)
  |/
  o  CL3: C ()
  |
  o  CL2: B ()
  |
  o  CL1: A ()
  

Confirm A has different content master/release
  $ hg cat -r master A
  A
  AA
  $ hg cat -r release A
  A
  D
  $ hg debugindex A
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       3     -1       0 45f17b21388f 000000000000 000000000000
       1         3       6     -1       3 4b71681c4c9d 45f17b21388f 000000000000
       2         9       5     -1       5 99c4909b2774 45f17b21388f 000000000000

Add file Z to release
  $ cd $p4wd
  $ echo Z > Z; p4 -c hg-p4-branch add Z
  //depot/Release/Z#1 - opened for add
  $ p4 -q -c hg-p4-branch submit -d Z

Add file Y to master
  $ echo Y > Y; p4 add Y
  //depot/Main/Y#1 - opened for add
  $ p4 -q submit -d Y

Update mater and release
  $ cd $hgwd
  $ hg p4seqimport -B release -P $P4ROOT hg-p4-branch
  $ hg p4seqimport -B master -P $P4ROOT hg-p4-import

Confirm master has Y and release has Z
  $ hg manifest -r master
  A
  B
  C
  Y
  $ hg manifest -r release
  A
  B
  C
  Z

Ensure commit graph is correct
  $ hg log -G -T 'CL{extras.p4changelist}: {desc} ({bookmarks})\n\n'
  o  CL8: Y (master)
  |
  | o  CL7: Z (release)
  | |
  | o  CL6: D ()
  | |
  | o  CL4: Populate //depot/Main/... //depot/Release/.... ()
  | |
  o |  CL5: AA ()
  |/
  o  CL3: C ()
  |
  o  CL2: B ()
  |
  o  CL1: A ()
  

End Test
  stopping the p4 server
