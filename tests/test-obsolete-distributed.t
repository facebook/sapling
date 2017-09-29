=============================
Test distributed obsolescence
=============================

This file test various cases where data (changeset, phase, obsmarkers) is
added to the repository in a specific order. Usually, this order is unlikely
to happen in the local case but can easily happen in the distributed case.

  $ unset HGUSER
  $ unset EMAIL
  $ . $TESTDIR/testlib/obsmarker-common.sh
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution = all
  > [phases]
  > publish = False
  > [templates]
  > obsfatesuccessors = "{if(successors, " as ")}{join(successors, ", ")}"
  > obsfateverb = "{obsfateverb(successors)}"
  > obsfateoperations = "{if(obsfateoperations(markers), " using {join(obsfateoperations(markers), ", ")}")}"
  > obsfateusers = "{if(obsfateusers(markers), " by {join(obsfateusers(markers), ", ")}")}"
  > obsfatedate = "{if(obsfatedate(markers), "{ifeq(min(obsfatedate(markers)), max(obsfatedate(markers)), " (at {min(obsfatedate(markers))|isodate})", " (between {min(obsfatedate(markers))|isodate} and {max(obsfatedate(markers))|isodate})")}")}"
  > obsfate = "{obsfateverb}{obsfateoperations}{obsfatesuccessors}{obsfateusers}{obsfatedate}; "
  > [ui]
  > logtemplate= {rev}:{node|short} {desc} {if(succsandmarkers, "[{succsandmarkers % "{obsfate}"}]")}\n
  > EOF

Check distributed chain building
================================

Test case where a changeset is marked as a successor of another local
changeset while the successor has already been obsoleted remotely.

The chain of evolution should seamlessly connect and all but the new version
(created remotely) should be seen as obsolete.

Initial setup

  $ mkdir distributed-chain-building
  $ cd distributed-chain-building
  $ hg init server
  $ cd server
  $ cat << EOF >> .hg/hgrc
  > [ui]
  > username = server
  > EOF
  $ mkcommit ROOT
  $ mkcommit c_A0
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit c_A1
  created new head
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit c_B0
  created new head
  $ hg debugobsolete `getid 'desc("c_A0")'` `getid 'desc("c_A1")'`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  3:e5d7dda7cd28 c_B0
  |
  | o  2:7f6b0a6f5c25 c_A1
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000); ]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  $ cd ..

duplicate the repo for the client:

  $ cp -R server client
  $ cat << EOF >> client/.hg/hgrc
  > [paths]
  > default = ../server/
  > [ui]
  > username = client
  > EOF

server side: create new revision on the server (obsoleting another one)

  $ cd server
  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit c_B1
  created new head
  $ hg debugobsolete `getid 'desc("c_B0")'` `getid 'desc("c_B1")'`
  obsoleted 1 changesets
  $ hg log -G
  @  4:391a2bf12b1b c_B1
  |
  | o  2:7f6b0a6f5c25 c_A1
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden
  @  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000); ]
  |/
  | o  2:7f6b0a6f5c25 c_A1
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000); ]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 391a2bf12b1b8b05a72400ae36b26d50a091dc22 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  $ cd ..

client side: create a marker between two common changesets
(client is not aware of the server activity yet)

  $ cd client
  $ hg debugobsolete `getid 'desc("c_A1")'` `getid 'desc("c_B0")'`
  obsoleted 1 changesets
  $ hg log -G
  @  3:e5d7dda7cd28 c_B0
  |
  o  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden
  @  3:e5d7dda7cd28 c_B0
  |
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000); ]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000); ]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  7f6b0a6f5c25345a83870963efd827c1798a5959 e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'client'}

client side: pull from the server
(the new successors should take over)

  $ hg up 'desc("ROOT")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg pull
  pulling from $TESTTMP/distributed-chain-building/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets
  (run 'hg heads' to see heads)
  $ hg log -G
  o  4:391a2bf12b1b c_B1
  |
  @  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden
  o  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000); ]
  |/
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000); ]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000); ]
  |/
  @  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  7f6b0a6f5c25345a83870963efd827c1798a5959 e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'client'}
  e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 391a2bf12b1b8b05a72400ae36b26d50a091dc22 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}

server side: receive client push
(the other way around, pushing to the server, the obsolete changesets stay
obsolete on the server side but the marker is sent out.)

  $ hg rollback
  repository tip rolled back to revision 3 (undo pull)
  $ hg push -f
  pushing to $TESTTMP/distributed-chain-building/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
  1 new obsolescence markers
  obsoleted 1 changesets
  $ hg -R ../server/ log -G
  @  4:391a2bf12b1b c_B1
  |
  o  0:e82fb8d02bbf ROOT
  
  $ hg -R ../server/ log -G --hidden
  @  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000); ]
  |/
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000); ]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000); ]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  7f6b0a6f5c25345a83870963efd827c1798a5959 e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'client'}
