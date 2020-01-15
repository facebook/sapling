#chg-compatible

  $ setconfig extensions.treemanifest=!
Check whether size of generaldelta revlog is not bigger than its
regular equivalent. Test would fail if generaldelta was naive
implementation of parentdelta: third manifest revision would be fully
inserted due to big distance from its paren revision (zero).

  $ hg init repo --config format.generaldelta=no --config format.usegeneraldelta=no
  $ cd repo
  $ echo foo > foo
  $ echo bar > bar
  $ echo baz > baz
  $ hg commit -q -Am boo
  $ hg clone --pull . ../gdrepo -q --config format.generaldelta=yes
  $ for r in 1 2 3; do
  >   echo $r > foo
  >   hg commit -q -m $r
  >   hg up -q -r 0
  >   hg pull . -q -r $r -R ../gdrepo
  > done

  $ cd ..
  >>> from __future__ import print_function
  >>> import os
  >>> regsize = os.stat("repo/.hg/store/00manifest.i").st_size
  >>> gdsize = os.stat("gdrepo/.hg/store/00manifest.i").st_size
  >>> if regsize < gdsize:
  ...     print('generaldata increased size of manifest')

Verify rev reordering doesnt create invalid bundles (issue4462)
This requires a commit tree that when pulled will reorder manifest revs such
that the second manifest to create a file rev will be ordered before the first
manifest to create that file rev. We also need to do a partial pull to ensure
reordering happens. At the end we verify the linkrev points at the earliest
commit.

  $ hg init server --config format.generaldelta=True
  $ cd server
  $ touch a
  $ hg commit -Aqm a
  $ echo x > x
  $ echo y > y
  $ hg commit -Aqm xy
  $ hg up -q '.^'
  $ echo x > x
  $ echo z > z
  $ hg commit -Aqm xz
  $ hg up -q 1
  $ echo b > b
  $ hg commit -Aqm b
  $ hg merge -q 2
  $ hg commit -Aqm merge
  $ echo c > c
  $ hg commit -Aqm c
  $ hg log -G -T '{rev} {shortest(node)} {desc}'
  @  5 ebb8 c
  |
  o    4 baf7 merge
  |\
  | o  3 a129 b
  | |
  o |  2 958c xz
  | |
  | o  1 f00c xy
  |/
  o  0 3903 a
  
  $ cd ..
  $ hg init client --config format.generaldelta=false --config format.usegeneraldelta=false
  $ cd client
  $ hg pull -q ../server -r 4
  $ hg debugindex x
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       3      0       1 1406e7411862 000000000000 000000000000

  $ cd ..

Test "usegeneraldelta" config
(repo are general delta, but incoming bundle are not re-deltafied)

delta coming from the server base delta server are not recompressed.
(also include the aggressive version for comparison)

  $ hg clone repo --pull --config format.usegeneraldelta=1 usegd
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 6 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone repo --pull --config format.generaldelta=1 full
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 6 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
#if common-zlib
  $ hg -R repo debugindex -m
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     104      0       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      0       2 134fdc6fd680 cef96823c800 000000000000
       3       218     104      3       3 723508934dad cef96823c800 000000000000
  $ hg -R usegd debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     104     -1       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      1       2 134fdc6fd680 cef96823c800 000000000000
       3       218      57      0       3 723508934dad cef96823c800 000000000000
  $ hg -R full debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     104     -1       0 cef96823c800 000000000000 000000000000
       1       104      57      0       1 58ab9a8d541d cef96823c800 000000000000
       2       161      57      0       2 134fdc6fd680 cef96823c800 000000000000
       3       218      57      0       3 723508934dad cef96823c800 000000000000
#endif

Test format.aggressivemergedeltas

  $ hg init --config format.generaldelta=1 aggressive
  $ cd aggressive
  $ cat << EOF >> .hg/hgrc
  > [format]
  > generaldelta = 1
  > EOF
  $ touch a b c d e
  $ hg commit -Aqm side1
  $ hg up -q null
  $ touch x y
  $ hg commit -Aqm side2

- Verify non-aggressive merge uses p1 (commit 1) as delta parent
  $ hg merge -q 0
  $ hg commit -q -m merge
#if common-zlib
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      59     -1       0 8dde941edb6e 000000000000 000000000000
       1        59      61      0       1 315c023f341d 000000000000 000000000000
       2       120      65      1       2 2ab389a983eb 315c023f341d 8dde941edb6e
#endif

  $ hg debugstrip -q -r .

- Verify aggressive merge uses p2 (commit 0) as delta parent
  $ hg up -q -C 1
  $ hg merge -q 0
  $ hg commit -q -m merge --config format.aggressivemergedeltas=True
#if common-zlib
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      59     -1       0 8dde941edb6e 000000000000 000000000000
       1        59      61      0       1 315c023f341d 000000000000 000000000000
       2       120      62      0       2 2ab389a983eb 315c023f341d 8dde941edb6e
#endif

Test that strip bundle use bundle2
  $ hg debugstrip .
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/aggressive/.hg/strip-backup/1c5d4dc9a8b8-6c68e60c-backup.hg
  $ hg debugbundle .hg/strip-backup/*
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 1, version: 02}
      1c5d4dc9a8b8d6e1750966d343e94db665e7a1e9
  phase-heads -- {}
      1c5d4dc9a8b8d6e1750966d343e94db665e7a1e9 draft

  $ cd ..

test maxdeltachainspan

  $ hg init source-repo
  $ cd source-repo
  $ hg debugbuilddag --new-file '.+5:brancha$.+11:branchb$.+30:branchc<brancha+2<branchb+2'
  $ cd ..
  $ hg -R source-repo debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      46     -1       0 19deeef41503 000000000000 000000000000
       1        46      57      0       1 fffc37b38c40 19deeef41503 000000000000
       2       103      57      1       2 5822d75c83d9 fffc37b38c40 000000000000
       3       160      57      2       3 19cf2273e601 5822d75c83d9 000000000000
       4       217      57      3       4 d45ead487afe 19cf2273e601 000000000000
       5       274      57      4       5 96e0c2ce55ed d45ead487afe 000000000000
       6       331      46     -1       6 0c2ea5222c74 000000000000 000000000000
       7       377      57      6       7 4ca08a89134d 0c2ea5222c74 000000000000
       8       434      57      7       8 c973dbfd30ac 4ca08a89134d 000000000000
       9       491      57      8       9 d81d878ff2cd c973dbfd30ac 000000000000
      10       548      58      9      10 dbee7f0dd760 d81d878ff2cd 000000000000
      11       606      58     10      11 474be9f1fd4e dbee7f0dd760 000000000000
      12       664      58     11      12 594a27502c85 474be9f1fd4e 000000000000
      13       722      58     12      13 a7d25307d6a9 594a27502c85 000000000000
      14       780      58     13      14 3eb53082272e a7d25307d6a9 000000000000
      15       838      58     14      15 d1e94c85caf6 3eb53082272e 000000000000
      16       896      58     15      16 8933d9629788 d1e94c85caf6 000000000000
      17       954      58     16      17 a33416e52d91 8933d9629788 000000000000
      18      1012      47     -1      18 4ccbf31021ed 000000000000 000000000000
      19      1059      58     18      19 dcad7a25656c 4ccbf31021ed 000000000000
      20      1117      58     19      20 617c4f8be75f dcad7a25656c 000000000000
      21      1175      58     20      21 975b9c1d75bb 617c4f8be75f 000000000000
      22      1233      58     21      22 74f09cd33b70 975b9c1d75bb 000000000000
      23      1291      58     22      23 54e79bfa7ef1 74f09cd33b70 000000000000
      24      1349      58     23      24 c556e7ff90af 54e79bfa7ef1 000000000000
      25      1407      58     24      25 42daedfe9c6b c556e7ff90af 000000000000
      26      1465      58     25      26 f302566947c7 42daedfe9c6b 000000000000
      27      1523      58     26      27 2346959851cb f302566947c7 000000000000
      28      1581      58     27      28 ca8d867106b4 2346959851cb 000000000000
      29      1639      58     28      29 fd9152decab2 ca8d867106b4 000000000000
      30      1697      58     29      30 3fe34080a79b fd9152decab2 000000000000
      31      1755      58     30      31 bce61a95078e 3fe34080a79b 000000000000
      32      1813      58     31      32 1dd9ba54ba15 bce61a95078e 000000000000
      33      1871      58     32      33 3cd9b90a9972 1dd9ba54ba15 000000000000
      34      1929      58     33      34 5db8c9754ef5 3cd9b90a9972 000000000000
      35      1987      58     34      35 ee4a240cc16c 5db8c9754ef5 000000000000
      36      2045      58     35      36 9e1d38725343 ee4a240cc16c 000000000000
      37      2103      58     36      37 3463f73086a8 9e1d38725343 000000000000
      38      2161      58     37      38 88af72fab449 3463f73086a8 000000000000
      39      2219      58     38      39 472f5ce73785 88af72fab449 000000000000
      40      2277      58     39      40 c91b8351e5b8 472f5ce73785 000000000000
      41      2335      58     40      41 9c8289c5c5c0 c91b8351e5b8 000000000000
      42      2393      58     41      42 a13fd4a09d76 9c8289c5c5c0 000000000000
      43      2451      58     42      43 2ec2c81cafe0 a13fd4a09d76 000000000000
      44      2509      58     43      44 f27fdd174392 2ec2c81cafe0 000000000000
      45      2567      58     44      45 a539ec59fe41 f27fdd174392 000000000000
      46      2625      58     45      46 5e98b9ecb738 a539ec59fe41 000000000000
      47      2683      58     46      47 31e6b47899d0 5e98b9ecb738 000000000000
      48      2741      58     47      48 2cf25d6636bd 31e6b47899d0 000000000000
      49      2799      58      5      49 9fff62ea0624 96e0c2ce55ed 000000000000
      50      2857      58     49      50 467f8e30a066 9fff62ea0624 000000000000
      51      2915      58     17      51 346db97283df a33416e52d91 000000000000
      52      2973      58     51      52 4e003fd4d5cd 346db97283df 000000000000
  $ hg clone --pull source-repo relax-chain --config format.generaldelta=yes
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 53 changesets with 53 changes to 53 files
  updating to branch default
  14 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R relax-chain debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      46     -1       0 19deeef41503 000000000000 000000000000
       1        46      57      0       1 fffc37b38c40 19deeef41503 000000000000
       2       103      57      1       2 5822d75c83d9 fffc37b38c40 000000000000
       3       160      57      2       3 19cf2273e601 5822d75c83d9 000000000000
       4       217      57      3       4 d45ead487afe 19cf2273e601 000000000000
       5       274      57      4       5 96e0c2ce55ed d45ead487afe 000000000000
       6       331      46     -1       6 0c2ea5222c74 000000000000 000000000000
       7       377      57      6       7 4ca08a89134d 0c2ea5222c74 000000000000
       8       434      57      7       8 c973dbfd30ac 4ca08a89134d 000000000000
       9       491      57      8       9 d81d878ff2cd c973dbfd30ac 000000000000
      10       548      58      9      10 dbee7f0dd760 d81d878ff2cd 000000000000
      11       606      58     10      11 474be9f1fd4e dbee7f0dd760 000000000000
      12       664      58     11      12 594a27502c85 474be9f1fd4e 000000000000
      13       722      58     12      13 a7d25307d6a9 594a27502c85 000000000000
      14       780      58     13      14 3eb53082272e a7d25307d6a9 000000000000
      15       838      58     14      15 d1e94c85caf6 3eb53082272e 000000000000
      16       896      58     15      16 8933d9629788 d1e94c85caf6 000000000000
      17       954      58     16      17 a33416e52d91 8933d9629788 000000000000
      18      1012      47     -1      18 4ccbf31021ed 000000000000 000000000000
      19      1059      58     18      19 dcad7a25656c 4ccbf31021ed 000000000000
      20      1117      58     19      20 617c4f8be75f dcad7a25656c 000000000000
      21      1175      58     20      21 975b9c1d75bb 617c4f8be75f 000000000000
      22      1233      58     21      22 74f09cd33b70 975b9c1d75bb 000000000000
      23      1291      58     22      23 54e79bfa7ef1 74f09cd33b70 000000000000
      24      1349      58     23      24 c556e7ff90af 54e79bfa7ef1 000000000000
      25      1407      58     24      25 42daedfe9c6b c556e7ff90af 000000000000
      26      1465      58     25      26 f302566947c7 42daedfe9c6b 000000000000
      27      1523      58     26      27 2346959851cb f302566947c7 000000000000
      28      1581      58     27      28 ca8d867106b4 2346959851cb 000000000000
      29      1639      58     28      29 fd9152decab2 ca8d867106b4 000000000000
      30      1697      58     29      30 3fe34080a79b fd9152decab2 000000000000
      31      1755      58     30      31 bce61a95078e 3fe34080a79b 000000000000
      32      1813      58     31      32 1dd9ba54ba15 bce61a95078e 000000000000
      33      1871      58     32      33 3cd9b90a9972 1dd9ba54ba15 000000000000
      34      1929      58     33      34 5db8c9754ef5 3cd9b90a9972 000000000000
      35      1987      58     34      35 ee4a240cc16c 5db8c9754ef5 000000000000
      36      2045      58     35      36 9e1d38725343 ee4a240cc16c 000000000000
      37      2103      58     36      37 3463f73086a8 9e1d38725343 000000000000
      38      2161      58     37      38 88af72fab449 3463f73086a8 000000000000
      39      2219      58     38      39 472f5ce73785 88af72fab449 000000000000
      40      2277      58     39      40 c91b8351e5b8 472f5ce73785 000000000000
      41      2335      58     40      41 9c8289c5c5c0 c91b8351e5b8 000000000000
      42      2393      58     41      42 a13fd4a09d76 9c8289c5c5c0 000000000000
      43      2451      58     42      43 2ec2c81cafe0 a13fd4a09d76 000000000000
      44      2509      58     43      44 f27fdd174392 2ec2c81cafe0 000000000000
      45      2567      58     44      45 a539ec59fe41 f27fdd174392 000000000000
      46      2625      58     45      46 5e98b9ecb738 a539ec59fe41 000000000000
      47      2683      58     46      47 31e6b47899d0 5e98b9ecb738 000000000000
      48      2741      58     47      48 2cf25d6636bd 31e6b47899d0 000000000000
      49      2799      58      5      49 9fff62ea0624 96e0c2ce55ed 000000000000
      50      2857      58     49      50 467f8e30a066 9fff62ea0624 000000000000
      51      2915      58     17      51 346db97283df a33416e52d91 000000000000
      52      2973      58     51      52 4e003fd4d5cd 346db97283df 000000000000
