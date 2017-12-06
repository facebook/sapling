Testing the functionality to pull remotenames
=============================================

  $ cat >> $HGRCPATH << EOF
  > [alias]
  > glog = log -G -T '{rev}:{node|short}  {desc}'
  > [experimental]
  > remotenames = True
  > EOF

Making a server repo
--------------------

  $ hg init server
  $ cd server
  $ for ch in a b c d e f g h; do
  >   echo "foo" >> $ch
  >   hg ci -Aqm "Added "$ch
  > done
  $ hg glog
  @  7:ec2426147f0e  Added h
  |
  o  6:87d6d6676308  Added g
  |
  o  5:825660c69f0c  Added f
  |
  o  4:aa98ab95a928  Added e
  |
  o  3:62615734edd5  Added d
  |
  o  2:28ad74487de9  Added c
  |
  o  1:29becc82797a  Added b
  |
  o  0:18d04c59bb5d  Added a
  
  $ hg bookmark -r 3 foo
  $ hg bookmark -r 6 bar
  $ hg up 4
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branch wat
  marked working directory as branch wat
  (branches are permanent and global, did you want a bookmark?)
  $ echo foo >> bar
  $ hg ci -Aqm "added bar"

Making a client repo
--------------------

  $ cd ..

  $ hg clone server client
  updating to branch default
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd client
  $ cat .hg/logexchange/bookmarks
  0
  
  87d6d66763085b629e6d7ed56778c79827273022\x00file:$TESTTMP/server\x00bar (esc)
  62615734edd52f06b6fb9c2beb429e4fe30d57b8\x00file:$TESTTMP/server\x00foo (esc)

  $ cat .hg/logexchange/branches
  0
  
  ec2426147f0e39dbc9cef599b066be6035ce691d\x00file:$TESTTMP/server\x00default (esc)
  3e1487808078543b0af6d10dadf5d46943578db0\x00file:$TESTTMP/server\x00wat (esc)

Making a new server
-------------------

  $ cd ..
  $ hg init server2
  $ cd server2
  $ hg pull ../server/
  pulling from ../server/
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 9 changes to 9 files (+1 heads)
  adding remote bookmark bar
  adding remote bookmark foo
  new changesets 18d04c59bb5d:3e1487808078
  (run 'hg heads' to see heads)

Pulling form the new server
---------------------------
  $ cd ../client/
  $ hg pull ../server2/
  pulling from ../server2/
  searching for changes
  no changes found
  $ cat .hg/logexchange/bookmarks
  0
  
  62615734edd52f06b6fb9c2beb429e4fe30d57b8\x00file:$TESTTMP/server\x00foo (esc)
  87d6d66763085b629e6d7ed56778c79827273022\x00file:$TESTTMP/server\x00bar (esc)
  87d6d66763085b629e6d7ed56778c79827273022\x00file:$TESTTMP/server2\x00bar (esc)
  62615734edd52f06b6fb9c2beb429e4fe30d57b8\x00file:$TESTTMP/server2\x00foo (esc)

  $ cat .hg/logexchange/branches
  0
  
  3e1487808078543b0af6d10dadf5d46943578db0\x00file:$TESTTMP/server\x00wat (esc)
  ec2426147f0e39dbc9cef599b066be6035ce691d\x00file:$TESTTMP/server\x00default (esc)
  ec2426147f0e39dbc9cef599b066be6035ce691d\x00file:$TESTTMP/server2\x00default (esc)
  3e1487808078543b0af6d10dadf5d46943578db0\x00file:$TESTTMP/server2\x00wat (esc)
