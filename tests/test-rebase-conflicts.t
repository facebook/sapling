  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}:{phase} '{desc}' {branches} {bookmarks}\n"
  > EOF

  $ hg init a
  $ cd a
  $ echo c1 >common
  $ hg add common
  $ hg ci -m C1

  $ echo c2 >>common
  $ hg ci -m C2

  $ echo c3 >>common
  $ hg ci -m C3

  $ hg up -q -C 1

  $ echo l1 >>extra
  $ hg add extra
  $ hg ci -m L1
  created new head

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ echo l3 >> extra2
  $ hg add extra2
  $ hg ci -m L3
  $ hg bookmark mybook

  $ hg phase --force --secret 4

  $ hg tglog
  @  5:secret 'L3'  mybook
  |
  o  4:secret 'L2'
  |
  o  3:draft 'L1'
  |
  | o  2:draft 'C3'
  |/
  o  1:draft 'C2'
  |
  o  0:draft 'C1'
  
Try to call --continue:

  $ hg rebase --continue
  abort: no rebase in progress
  [255]

Conflicting rebase:

  $ hg rebase -s 3 -d 2
  rebasing 3:3163e20567cc "L1"
  rebasing 4:46f0b057b5c0 "L2"
  merging common
  warning: conflicts during merge.
  merging common incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Try to continue without solving the conflict:

  $ hg rebase --continue
  already rebased 3:3163e20567cc "L1" as 3e046f2ecedb
  rebasing 4:46f0b057b5c0 "L2"
  abort: unresolved merge conflicts (see "hg help resolve")
  [255]

Conclude rebase:

  $ echo 'resolved merge' >common
  $ hg resolve -m common
  (no more unresolved files)
  $ hg rebase --continue
  already rebased 3:3163e20567cc "L1" as 3e046f2ecedb
  rebasing 4:46f0b057b5c0 "L2"
  rebasing 5:8029388f38dc "L3" (mybook)
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/3163e20567cc-5ca4656e-backup.hg (glob)

  $ hg tglog
  @  5:secret 'L3'  mybook
  |
  o  4:secret 'L2'
  |
  o  3:draft 'L1'
  |
  o  2:draft 'C3'
  |
  o  1:draft 'C2'
  |
  o  0:draft 'C1'
  
Check correctness:

  $ hg cat -r 0 common
  c1

  $ hg cat -r 1 common
  c1
  c2

  $ hg cat -r 2 common
  c1
  c2
  c3

  $ hg cat -r 3 common
  c1
  c2
  c3

  $ hg cat -r 4 common
  resolved merge

  $ hg cat -r 5 common
  resolved merge

Bookmark stays active after --continue
  $ hg bookmarks
   * mybook                    5:d67b21408fc0

  $ cd ..

Check that the right ancestors is used while rebasing a merge (issue4041)

  $ hg clone "$TESTDIR/bundles/issue4041.hg" issue4041
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 11 changesets with 8 changes to 3 files (+1 heads)
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd issue4041
  $ hg log -G
  o    changeset:   10:2f2496ddf49d
  |\   branch:      f1
  | |  tag:         tip
  | |  parent:      7:4c9fbe56a16f
  | |  parent:      9:e31216eec445
  | |  user:        szhang
  | |  date:        Thu Sep 05 12:59:39 2013 -0400
  | |  summary:     merge
  | |
  | o  changeset:   9:e31216eec445
  | |  branch:      f1
  | |  user:        szhang
  | |  date:        Thu Sep 05 12:59:10 2013 -0400
  | |  summary:     more changes to f1
  | |
  | o    changeset:   8:8e4e2c1a07ae
  | |\   branch:      f1
  | | |  parent:      2:4bc80088dc6b
  | | |  parent:      6:400110238667
  | | |  user:        szhang
  | | |  date:        Thu Sep 05 12:57:59 2013 -0400
  | | |  summary:     bad merge
  | | |
  o | |  changeset:   7:4c9fbe56a16f
  |/ /   branch:      f1
  | |    parent:      2:4bc80088dc6b
  | |    user:        szhang
  | |    date:        Thu Sep 05 12:54:00 2013 -0400
  | |    summary:     changed f1
  | |
  | o  changeset:   6:400110238667
  | |  branch:      f2
  | |  parent:      4:12e8ec6bb010
  | |  user:        szhang
  | |  date:        Tue Sep 03 13:58:02 2013 -0400
  | |  summary:     changed f2 on f2
  | |
  | | @  changeset:   5:d79e2059b5c0
  | | |  parent:      3:8a951942e016
  | | |  user:        szhang
  | | |  date:        Tue Sep 03 13:57:39 2013 -0400
  | | |  summary:     changed f2 on default
  | | |
  | o |  changeset:   4:12e8ec6bb010
  | |/   branch:      f2
  | |    user:        szhang
  | |    date:        Tue Sep 03 13:57:18 2013 -0400
  | |    summary:     created f2 branch
  | |
  | o  changeset:   3:8a951942e016
  | |  parent:      0:24797d4f68de
  | |  user:        szhang
  | |  date:        Tue Sep 03 13:57:11 2013 -0400
  | |  summary:     added f2.txt
  | |
  o |  changeset:   2:4bc80088dc6b
  | |  branch:      f1
  | |  user:        szhang
  | |  date:        Tue Sep 03 13:56:20 2013 -0400
  | |  summary:     added f1.txt
  | |
  o |  changeset:   1:ef53c9e6b608
  |/   branch:      f1
  |    user:        szhang
  |    date:        Tue Sep 03 13:55:26 2013 -0400
  |    summary:     created f1 branch
  |
  o  changeset:   0:24797d4f68de
     user:        szhang
     date:        Tue Sep 03 13:55:08 2013 -0400
     summary:     added default.txt
  
  $ hg rebase -s9 -d2 --debug # use debug to really check merge base used
  rebase onto 2 starting from e31216eec445
  ignoring null merge rebase of 3
  ignoring null merge rebase of 4
  ignoring null merge rebase of 6
  ignoring null merge rebase of 8
  rebasing 9:e31216eec445 "more changes to f1"
   future parents are 2 and -1
  rebase status stored
   update to 2:4bc80088dc6b
  resolving manifests
   branchmerge: False, force: True, partial: False
   ancestor: d79e2059b5c0+, local: d79e2059b5c0+, remote: 4bc80088dc6b
   f2.txt: other deleted -> r
  removing f2.txt
   f1.txt: remote created -> g
  getting f1.txt
   merge against 9:e31216eec445
     detach base 8:8e4e2c1a07ae
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 8e4e2c1a07ae, local: 4bc80088dc6b+, remote: e31216eec445
   f1.txt: remote is newer -> g
  getting f1.txt
  committing files:
  f1.txt
  committing manifest
  committing changelog
  rebased as 19c888675e13
  rebasing 10:2f2496ddf49d "merge" (tip)
   future parents are 11 and 7
  rebase status stored
   already in target
   merge against 10:2f2496ddf49d
     detach base 9:e31216eec445
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: e31216eec445, local: 19c888675e13+, remote: 2f2496ddf49d
   f1.txt: remote is newer -> g
  getting f1.txt
  committing files:
  f1.txt
  committing manifest
  committing changelog
  rebased as 2a7f09cac94c
  rebase merging completed
  update back to initial working directory parent
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 2a7f09cac94c, local: 2a7f09cac94c+, remote: d79e2059b5c0
   f1.txt: other deleted -> r
  removing f1.txt
   f2.txt: remote created -> g
  getting f2.txt
  2 changesets found
  list of changesets:
  e31216eec445e44352c5f01588856059466a24c9
  2f2496ddf49d69b5ef23ad8cf9fb2e0e4faf0ac2
  saved backup bundle to $TESTTMP/issue4041/.hg/strip-backup/e31216eec445-15f7a814-backup.hg (glob)
  3 changesets found
  list of changesets:
  4c9fbe56a16f30c0d5dcc40ec1a97bbe3325209c
  19c888675e133ab5dff84516926a65672eaf04d9
  2a7f09cac94c7f4b73ebd5cd1a62d3b2e8e336bf
  adding branch
  adding changesets
  add changeset 4c9fbe56a16f
  add changeset 19c888675e13
  add changeset 2a7f09cac94c
  adding manifests
  adding file changes
  adding f1.txt revisions
  added 2 changesets with 2 changes to 1 files
  invalid branchheads cache (served): tip differs
  rebase completed
  updating the branch cache
  truncating cache/rbc-revs-v1 to 72
