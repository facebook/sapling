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
  > [extensions]
  > rebase =
  > [experimental]
  > evolution = all
  > [phases]
  > publish = False
  > [ui]
  > logtemplate= {rev}:{node|short} {desc}{if(obsfate, " [{join(obsfate, "; ")}]")}\n
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
  $ hg log -G --hidden -v
  @  3:e5d7dda7cd28 c_B0
  |
  | o  2:7f6b0a6f5c25 c_A1
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000)]
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
  
  $ hg log -G --hidden -v
  @  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000)]
  |/
  | o  2:7f6b0a6f5c25 c_A1
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000)]
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
  
  $ hg log -G --hidden -v
  @  3:e5d7dda7cd28 c_B0
  |
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000)]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000)]
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
  new changesets 391a2bf12b1b
  (run 'hg heads' to see heads)
  $ hg log -G
  o  4:391a2bf12b1b c_B1
  |
  @  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden -v
  o  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000)]
  |/
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000)]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000)]
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
  
  $ hg -R ../server/ log -G --hidden -v
  @  4:391a2bf12b1b c_B1
  |
  | x  3:e5d7dda7cd28 c_B0 [rewritten as 4:391a2bf12b1b by server (at 1970-01-01 00:00 +0000)]
  |/
  | x  2:7f6b0a6f5c25 c_A1 [rewritten as 3:e5d7dda7cd28 by client (at 1970-01-01 00:00 +0000)]
  |/
  | x  1:e1b46f0f979f c_A0 [rewritten as 2:7f6b0a6f5c25 by server (at 1970-01-01 00:00 +0000)]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  e1b46f0f979f52748347ff8729c59f2ef56e6fe2 7f6b0a6f5c25345a83870963efd827c1798a5959 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'server'}
  7f6b0a6f5c25345a83870963efd827c1798a5959 e5d7dda7cd28e6b3f79437e5b8122a38ece0255c 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'client'}
  $ cd ..

Check getting changesets after getting the markers
=================================================

This test case covers the scenario where commits are received -after- we
received some obsolescence markers turning them obsolete.

For example, we pull some successors from a repository (with associated
predecessors marker chain) and then later we pull some intermediate
precedessors changeset from another repository. Obsolescence markers must
apply to the intermediate changeset. They have to be obsolete (and hidden).

Avoiding pulling the changeset in the first place is a tricky decision because
there could be non-obsolete ancestors that need to be pulled, but the
discovery cannot currently find these (this is not the case in this tests). In
addition, we could also have to pull the changeset because they have children.
In this case, they would not be hidden (yet) because of the orphan descendant,
but they would still have to be obsolete. (This is not tested in this case
either).

  $ mkdir distributed-chain-building
  $ cd distributed-chain-building
  $ hg init server
  $ cd server
  $ cat << EOF >> .hg/hgrc
  > [ui]
  > username = server
  > EOF
  $ mkcommit ROOT
  $ cd ..
  $ hg clone server repo-Alice
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF >> repo-Alice/.hg/hgrc
  > [ui]
  > username = alice
  > EOF
  $ hg clone server repo-Bob
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF >> repo-Bob/.hg/hgrc
  > [ui]
  > username = bob
  > EOF
  $ hg clone server repo-Celeste
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF >> repo-Celeste/.hg/hgrc
  > [ui]
  > username = celeste
  > EOF

Create some changesets locally

  $ cd repo-Alice
  $ mkcommit c_A0
  $ mkcommit c_B0
  $ cd ..

Bob pulls from Alice and rewrites them

  $ cd repo-Bob
  $ hg pull ../repo-Alice
  pulling from ../repo-Alice
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets d33b0a3a6464:ef908e42ce65
  (run 'hg update' to get a working copy)
  $ hg up 'desc("c_A")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit --amend -m 'c_A1'
  $ hg rebase -r 'desc("c_B0")' -d . # no easy way to rewrite the message with the rebase
  rebasing 2:ef908e42ce65 "c_B0"
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit --amend -m 'c_B1'
  $ hg log -G
  @  5:956063ac4557 c_B1
  |
  o  3:5b5708a437f2 c_A1
  |
  o  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden -v
  @  5:956063ac4557 c_B1
  |
  | x  4:5ffb9e311b35 c_B0 [rewritten using amend as 5:956063ac4557 by bob (at 1970-01-01 00:00 +0000)]
  |/
  o  3:5b5708a437f2 c_A1
  |
  | x  2:ef908e42ce65 c_B0 [rewritten using rebase as 4:5ffb9e311b35 by bob (at 1970-01-01 00:00 +0000)]
  | |
  | x  1:d33b0a3a6464 c_A0 [rewritten using amend as 3:5b5708a437f2 by bob (at 1970-01-01 00:00 +0000)]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  $ cd ..

Celeste pulls from Bob and rewrites them again

  $ cd repo-Celeste
  $ hg pull ../repo-Bob
  pulling from ../repo-Bob
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  3 new obsolescence markers
  new changesets 5b5708a437f2:956063ac4557
  (run 'hg update' to get a working copy)
  $ hg up 'desc("c_A")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit --amend -m 'c_A2'
  $ hg rebase -r 'desc("c_B1")' -d . # no easy way to rewrite the message with the rebase
  rebasing 2:956063ac4557 "c_B1"
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit --amend -m 'c_B2'
  $ hg log -G
  @  5:77ae25d99ff0 c_B2
  |
  o  3:9866d64649a5 c_A2
  |
  o  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden -v
  @  5:77ae25d99ff0 c_B2
  |
  | x  4:3cf8de21cc22 c_B1 [rewritten using amend as 5:77ae25d99ff0 by celeste (at 1970-01-01 00:00 +0000)]
  |/
  o  3:9866d64649a5 c_A2
  |
  | x  2:956063ac4557 c_B1 [rewritten using rebase as 4:3cf8de21cc22 by celeste (at 1970-01-01 00:00 +0000)]
  | |
  | x  1:5b5708a437f2 c_A1 [rewritten using amend as 3:9866d64649a5 by celeste (at 1970-01-01 00:00 +0000)]
  |/
  o  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}
  5b5708a437f27665db42c5a261a539a1bcb2a8c2 9866d64649a5d9c5991fe119c7b2c33898114e10 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  956063ac4557828781733b2d5677a351ce856f59 3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'celeste'}
  3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 77ae25d99ff07889e181126b1171b94bec8e5227 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}

Celeste now pushes to the server

(note: it would be enough to just have direct Celeste -> Alice exchange here.
However using a central server seems more common)

  $ hg push
  pushing to $TESTTMP/distributed-chain-building/distributed-chain-building/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  6 new obsolescence markers
  $ cd ..

Now Alice pulls from the server, then from Bob

Alice first retrieves the new evolution of its changesets and associated markers
from the server (note: could be from Celeste directly)

  $ cd repo-Alice
  $ hg up 'desc(ROOT)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg pull
  pulling from $TESTTMP/distributed-chain-building/distributed-chain-building/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 2 files (+1 heads)
  6 new obsolescence markers
  obsoleted 2 changesets
  new changesets 9866d64649a5:77ae25d99ff0
  (run 'hg heads' to see heads)
  $ hg debugobsolete
  3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 77ae25d99ff07889e181126b1171b94bec8e5227 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5b5708a437f27665db42c5a261a539a1bcb2a8c2 9866d64649a5d9c5991fe119c7b2c33898114e10 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  956063ac4557828781733b2d5677a351ce856f59 3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'celeste'}
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}

Then, she pulls from Bob, pulling predecessors of the changeset she has
already pulled. The changesets are not obsoleted in the Bob repo yet. Their
successors do not exist in Bob repository yet.

  $ hg pull ../repo-Bob
  pulling from ../repo-Bob
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads)
  $ hg log -G
  o  4:77ae25d99ff0 c_B2
  |
  o  3:9866d64649a5 c_A2
  |
  @  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 77ae25d99ff07889e181126b1171b94bec8e5227 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5b5708a437f27665db42c5a261a539a1bcb2a8c2 9866d64649a5d9c5991fe119c7b2c33898114e10 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  956063ac4557828781733b2d5677a351ce856f59 3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'celeste'}
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}

Same tests, but change coming from a bundle
(testing with a bundle is interesting because absolutely no discovery or
decision is made in that case, so receiving the changesets are not an option).

  $ hg rollback
  repository tip rolled back to revision 4 (undo pull)
  $ hg log -G
  o  4:77ae25d99ff0 c_B2
  |
  o  3:9866d64649a5 c_A2
  |
  @  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 77ae25d99ff07889e181126b1171b94bec8e5227 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5b5708a437f27665db42c5a261a539a1bcb2a8c2 9866d64649a5d9c5991fe119c7b2c33898114e10 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  956063ac4557828781733b2d5677a351ce856f59 3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'celeste'}
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}
  $ hg -R ../repo-Bob bundle ../step-1.hg
  searching for changes
  2 changesets found
  $ hg unbundle ../step-1.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads)
  $ hg log -G
  o  4:77ae25d99ff0 c_B2
  |
  o  3:9866d64649a5 c_A2
  |
  @  0:e82fb8d02bbf ROOT
  
  $ hg log -G --hidden -v
  x  6:956063ac4557 c_B1 [rewritten using amend, rebase as 4:77ae25d99ff0 by celeste (at 1970-01-01 00:00 +0000)]
  |
  x  5:5b5708a437f2 c_A1 [rewritten using amend as 3:9866d64649a5 by celeste (at 1970-01-01 00:00 +0000)]
  |
  | o  4:77ae25d99ff0 c_B2
  | |
  | o  3:9866d64649a5 c_A2
  |/
  | x  2:ef908e42ce65 c_B0 [rewritten using amend, rebase as 6:956063ac4557 by bob (at 1970-01-01 00:00 +0000)]
  | |
  | x  1:d33b0a3a6464 c_A0 [rewritten using amend as 5:5b5708a437f2 by bob (at 1970-01-01 00:00 +0000)]
  |/
  @  0:e82fb8d02bbf ROOT
  
  $ hg debugobsolete
  3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 77ae25d99ff07889e181126b1171b94bec8e5227 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5b5708a437f27665db42c5a261a539a1bcb2a8c2 9866d64649a5d9c5991fe119c7b2c33898114e10 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'celeste'}
  5ffb9e311b35f6ab6f76f667ca5d6e595645481b 956063ac4557828781733b2d5677a351ce856f59 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  956063ac4557828781733b2d5677a351ce856f59 3cf8de21cc2282186857d2266eb6b1f9cb85ecf3 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'celeste'}
  d33b0a3a64647d79583526be8107802b1f9fedfa 5b5708a437f27665db42c5a261a539a1bcb2a8c2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'bob'}
  ef908e42ce65ef57f970d799acaddde26f58a4cc 5ffb9e311b35f6ab6f76f667ca5d6e595645481b 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'bob'}

  $ cd ..
