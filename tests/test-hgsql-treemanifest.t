
  $ CACHEDIR=`pwd`/hgcache
  $ . "$TESTDIR/hgsql/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > [treemanifest]
  > treeonly=False
  > EOF

Test that treemanifest backfill populates the database

  $ initserver master master
  $ initserver master-alreadysynced master
  $ initserver master-new master
  $ cd master
  $ touch a && hg ci -Aqm a
  $ mkdir dir
  $ touch dir/b && hg ci -Aqm b
  $ hg book master

  $ cd ../master-alreadysynced
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ hg log -r tip --forcesync -T '{rev}\n'
  1

  $ cd ../master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ DBGD=1 hg backfilltree
  $ ls .hg/store/meta/dir
  00manifest.i

Test that an empty repo syncs the tree revlogs

  $ cd ../master-new
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ hg log -r tip --forcesync -T '{rev}\n'
  1
  $ ls .hg/store/meta/dir
  00manifest.i

Test that we can replay backfills into an existing repo
  $ cd ../master-alreadysynced
  $ hg sqlreplay
  $ ls .hg/store/meta/dir
  00manifest.i
  $ rm -rf .hg/store/00manifesttree* .hg/store/meta
  $ hg sqlreplay --start 0 --end 0
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
  $ hg sqlreplay --start 1 --end 2
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
  $ cd ..

Test that trees created during push are synced to the db

  $ initclient client
  $ cd client
  $ hg pull -q ssh://user@dummy/master
  $ hg up -q tip
  $ touch dir/c && hg ci -Aqm c

  $ hg push ssh://user@dummy/master --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changeset:
  remote:     c46827e4453c  c

  $ cd ../master-new
  $ hg log -G -T '{rev} {desc}' --forcesync
  o  2 c
  |
  o  1 b
  |
  o  0 a
  
  $ hg debugdata .hg/store/meta/dir/00manifest.i 1
  b\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
  c\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)

Test that sqltreestrip deletes trees from history
  $ cd ../client
  $ mkdir dir2
  $ echo >> dir2/d && hg ci -Aqm d
  $ echo >> dir2/d && hg ci -Aqm d2
  $ hg push ssh://user@dummy/master --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 2 changesets:
  remote:     b3adfc03d09d  d
  remote:     fc50e1c24ca2  d2

  $ hg -R ../master-alreadysynced log -r tip --forcesync > /dev/null

  $ cd ../master
  $ hg log -G -T '{rev} {desc}' --forcesync
  o  4 d2
  |
  o  3 d
  |
  o  2 c
  |
  @  1 b
  |
  o  0 a
  

# First strip just the root treemanifest
  $ hg sqltreestrip 2 --i-know-what-i-am-doing --root-only
  *** YOU ARE ABOUT TO DELETE TREE HISTORY INCLUDING AND AFTER 2 (MANDATORY 5 SECOND WAIT) ***
  mysql: deleting root trees with linkrevs >= 2
  local: deleting root trees with linkrevs >= 2
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
  $ hg debugindex .hg/store/meta/dir2/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       3 d0729cbab2a9 000000000000 000000000000
       1        44      44     -1       4 cc280c5b788f d0729cbab2a9 000000000000

# Then strip all treemanifests
  $ hg sqltreestrip 2 --i-know-what-i-am-doing
  *** YOU ARE ABOUT TO DELETE TREE HISTORY INCLUDING AND AFTER 2 (MANDATORY 5 SECOND WAIT) ***
  mysql: deleting trees with linkrevs >= 2
  local: deleting trees with linkrevs >= 2
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
  $ hg debugindex .hg/store/meta/dir2/00manifest.i
     rev    offset  length   base linkrev nodeid       p1           p2
  $ hg debugindex .hg/store/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      59      0       1 898d94054864 8515d4bfda76 000000000000
       2       103      59      1       2 7cdc42a14359 898d94054864 000000000000
       3       162      60      2       3 0c96405fb5c3 7cdc42a14359 000000000000
       4       222      60      3       4 8b833dfa4cc5 0c96405fb5c3 000000000000
  $ hg status --change 4 --config treemanifest.treeonly=True
  abort: "unable to find the following nodes locally or on the server: ('', 8b833dfa4cc566bfd4bcb4d85e4a128be5e49334)"
  [255]

Refill the repository from the non-stripped master
  $ cd ../master-alreadysynced
  $ hg debugindex .hg/store/meta/dir2/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       3 d0729cbab2a9 000000000000 000000000000
       1        44      44     -1       4 cc280c5b788f d0729cbab2a9 000000000000
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
       2       102      58      1       2 7cdc42a14359 898d94054864 000000000000
       3       160      59      2       3 0c96405fb5c3 7cdc42a14359 000000000000
       4       219     *     -1       4 8b833dfa4cc5 0c96405fb5c3 000000000000 (glob)
  $ hg debugdata .hg/store/meta/dir2/00manifest.i 1
  d\x0028ad8a7cbb9ee7a7f5f50d46539b8dab63835959 (esc)
  $ hg sqlverify
  corruption: 'meta/dir/00manifest.i:f90dfbe4b2fd56ac55a98b322cf4dd420c5c07e5' with linkrev 2 exists on local disk, but not in sql
  corruption: 'meta/dir2/00manifest.i:cc280c5b788f79ee2ec4479fc2e3daa3972dc0af' with linkrev 4 exists on local disk, but not in sql
  corruption: 'meta/dir2/00manifest.i:d0729cbab2a9dece7b82fc241de6a62ecdd4a8b7' with linkrev 3 exists on local disk, but not in sql
  corruption: '00manifesttree.i:8b833dfa4cc566bfd4bcb4d85e4a128be5e49334' with linkrev 4 exists on local disk, but not in sql
  corruption: '00manifesttree.i:0c96405fb5c3fa57c048560e68bf33b87058ca1d' with linkrev 3 exists on local disk, but not in sql
  corruption: '00manifesttree.i:7cdc42a143599f196ad3e5e6e2dd5a1f78475d82' with linkrev 2 exists on local disk, but not in sql
  abort: Verification failed
  [255]
  $ hg sqlrefill --i-know-what-i-am-doing 1
  $ hg sqlverify
  Verification passed
  $ cd ../master
  $ hg sqlreplay
  $ hg debugindex .hg/store/meta/dir2/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       3 d0729cbab2a9 000000000000 000000000000
       1        44      44     -1       4 cc280c5b788f d0729cbab2a9 000000000000
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
       2       102      58      1       2 7cdc42a14359 898d94054864 000000000000
       3       160      59      2       3 0c96405fb5c3 7cdc42a14359 000000000000
       4       219     *     -1       4 8b833dfa4cc5 0c96405fb5c3 000000000000 (glob)
  $ hg debugdata .hg/store/meta/dir2/00manifest.i 1
  d\x0028ad8a7cbb9ee7a7f5f50d46539b8dab63835959 (esc)
  $ hg status --change 4 --config treemanifest.treeonly=True
  M dir2/d

# Restrip
  $ hg sqltreestrip 2 --i-know-what-i-am-doing
  *** YOU ARE ABOUT TO DELETE TREE HISTORY INCLUDING AND AFTER 2 (MANDATORY 5 SECOND WAIT) ***
  mysql: deleting trees with linkrevs >= 2
  local: deleting trees with linkrevs >= 2

Test local only strip
  $ cd ../master-alreadysynced
  $ hg sqltreestrip 2 --local-only --i-know-what-i-am-doing
  *** YOU ARE ABOUT TO DELETE TREE HISTORY INCLUDING AND AFTER 2 (MANDATORY 5 SECOND WAIT) ***
  local: deleting trees with linkrevs >= 2
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000

Refill trees in sql
(glob in the debugindex is because of different compression behavior in
different environments)
  $ cd ../master
  $ hg backfilltree
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
       2       102      58      1       2 7cdc42a14359 898d94054864 000000000000
       3       160      59      2       3 0c96405fb5c3 7cdc42a14359 000000000000
       4       219     *     -1       4 8b833dfa4cc5 0c96405fb5c3 000000000000 (glob)
  $ hg status --change 4 --config treemanifest.treeonly=True
  M dir2/d

Refill trees in the other master
  $ cd ../master-alreadysynced
  $ hg sqlreplay 2
  $ hg status --change 4 --config treemanifest.treeonly=True
  M dir2/d
  $ cd ..

Test that trees are written in linkrev order
  $ initserver ordermaster ordermaster
  $ cat >> ordermaster/.hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > treeonly=True
  > EOF

  $ initclient order-client
  $ cd order-client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > treeonly=True
  > sendtrees=True
  > [extensions]
  > remotenames=
  > EOF

  $ echo a >> a
  $ hg commit -Aqm A
  $ hg up null -q
  $ echo b >> b
  $ hg commit -Aqm B
  $ hg up null -q
  $ echo c >> c
  $ hg commit -Aqm C
  $ hg up -q 0
  $ hg merge -q 2
  $ hg commit -Aqm Merge1
  $ hg merge -q 1
  $ hg commit -Aqm Merge2

  $ hg push --config extensions.pushrebase=! --to master -q ssh://user@dummy/ordermaster --create

  $ cd ../ordermaster
# These should be in linkrev order after pushing to hgsql
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 a0c8bcbbb45c 000000000000 000000000000
       1        44      44     -1       1 23226e7a252c 000000000000 000000000000
       2        88      44     -1       2 86d7088ee657 000000000000 000000000000
       3       132      54      2       3 6c51dc0bfc37 a0c8bcbbb45c 86d7088ee657
       4       186      55      3       4 d2c02f8cb06c 6c51dc0bfc37 23226e7a252c
