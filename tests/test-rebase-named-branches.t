  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
  > EOF

  $ hg init a
  $ cd a
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

  $ hg clone -q -u . a a1

  $ cd a1

  $ hg update 3
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg branch dev-one
  marked working directory as branch dev-one
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'dev-one named branch'

  $ hg update 7
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branch dev-two
  marked working directory as branch dev-two

  $ echo x > x

  $ hg add x

  $ hg ci -m 'dev-two named branch'

  $ hg tglog
  @  9: 'dev-two named branch' dev-two
  |
  | o  8: 'dev-one named branch' dev-one
  | |
  o |  7: 'H'
  | |
  +---o  6: 'G'
  | | |
  o | |  5: 'F'
  | | |
  +---o  4: 'E'
  | |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  

Branch name containing a dash (issue3181)

  $ hg rebase -b dev-two -d dev-one --keepbranches
  rebasing 5:24b6387c8c8c "F"
  rebasing 6:eea13746799a "G"
  rebasing 7:02de42196ebe "H"
  rebasing 9:cb039b7cae8e "dev-two named branch" (tip)
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/24b6387c8c8c-24cb8001-backup.hg (glob)

  $ hg tglog
  @  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  o |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  | |
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s dev-one -d 0 --keepbranches
  rebasing 5:643fc9128048 "dev-one named branch"
  rebasing 6:24de4aff8e28 "F"
  rebasing 7:4b988a958030 "G"
  rebasing 8:31d0e4ba75e6 "H"
  rebasing 9:9e70cd31750f "dev-two named branch" (tip)
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/643fc9128048-c4ee9ef5-backup.hg (glob)

  $ hg tglog
  @  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  o |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg update 3
  3 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branch -f dev-one
  marked working directory as branch dev-one
  $ hg ci -m 'dev-one named branch'
  created new head

  $ hg tglog
  @  10: 'dev-one named branch' dev-one
  |
  | o  9: 'dev-two named branch' dev-two
  | |
  | o  8: 'H'
  | |
  | | o  7: 'G'
  | |/|
  | o |  6: 'F'
  | | |
  | o |  5: 'dev-one named branch' dev-one
  | | |
  | | o  4: 'E'
  | |/
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -b 'max(branch("dev-two"))' -d dev-one --keepbranches
  rebasing 5:bc8139ee757c "dev-one named branch"
  note: rebase of 5:bc8139ee757c created no changes to commit
  rebasing 6:42aa3cf0fa7a "F"
  rebasing 7:1a1e6f72ec38 "G"
  rebasing 8:904590360559 "H"
  rebasing 9:59c2e59309fe "dev-two named branch"
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/bc8139ee757c-f11c1080-backup.hg (glob)

  $ hg tglog
  o  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  @ |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  | |
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 'max(branch("dev-one"))' -d 0 --keepbranches
  rebasing 5:643fc9128048 "dev-one named branch"
  rebasing 6:679f28760620 "F"
  rebasing 7:549f007a9f5f "G"
  rebasing 8:12b2bc666e20 "H"
  rebasing 9:71325f8bc082 "dev-two named branch" (tip)
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/643fc9128048-6cdd1a52-backup.hg (glob)

  $ hg tglog
  o  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  @ |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg up -r 0 > /dev/null

Rebasing descendant onto ancestor across different named branches

  $ hg rebase -s 1 -d 9 --keepbranches
  rebasing 1:42ccdea3bb16 "B"
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/42ccdea3bb16-3cb021d3-backup.hg (glob)

  $ hg tglog
  o  9: 'D'
  |
  o  8: 'C'
  |
  o  7: 'B'
  |
  o  6: 'dev-two named branch' dev-two
  |
  o  5: 'H'
  |
  | o  4: 'G'
  |/|
  o |  3: 'F'
  | |
  o |  2: 'dev-one named branch' dev-one
  | |
  | o  1: 'E'
  |/
  @  0: 'A'
  
  $ hg rebase -s 5 -d 6
  abort: source is ancestor of destination
  [255]

  $ hg rebase -s 6 -d 5
  rebasing 6:3944801ae4ea "dev-two named branch"
  rebasing 7:3bdb949809d9 "B"
  rebasing 8:a0d543090fa4 "C"
  rebasing 9:e9f862ce8bad "D" (tip)
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/3944801ae4ea-fb46ed74-backup.hg (glob)

  $ hg tglog
  o  9: 'D'
  |
  o  8: 'C'
  |
  o  7: 'B'
  |
  o  6: 'dev-two named branch'
  |
  o  5: 'H'
  |
  | o  4: 'G'
  |/|
  o |  3: 'F'
  | |
  o |  2: 'dev-one named branch' dev-one
  | |
  | o  1: 'E'
  |/
  @  0: 'A'
  

Reopen branch by rebase

  $ hg up -qr3
  $ hg branch -q b
  $ hg ci -m 'create b'
  $ hg ci -m 'close b' --close
  $ hg rebase -b 8 -d b
  reopening closed branch head 2b586e70108d
  rebasing 5:8e279d293175 "H"
  rebasing 6:c57724c84928 "dev-two named branch"
  rebasing 7:160b0930ccc6 "B"
  rebasing 8:810110211f50 "C"
  rebasing 9:e522577ccdbd "D"
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/8e279d293175-b023e27c-backup.hg (glob)

  $ cd ..

Rebase to other head on branch

Set up a case:

  $ hg init case1
  $ cd case1
  $ touch f
  $ hg ci -qAm0
  $ hg branch -q b
  $ echo >> f
  $ hg ci -qAm 'b1'
  $ hg up -qr -2
  $ hg branch -qf b
  $ hg ci -qm 'b2'
  $ hg up -qr -3
  $ hg branch -q c
  $ hg ci -m 'c1'

  $ hg tglog
  @  3: 'c1' c
  |
  | o  2: 'b2' b
  |/
  | o  1: 'b1' b
  |/
  o  0: '0'
  
  $ hg clone -q . ../case2

rebase 'b2' to another lower branch head

  $ hg up -qr 2
  $ hg rebase
  rebasing 2:792845bb77ee "b2"
  note: rebase of 2:792845bb77ee created no changes to commit
  saved backup bundle to $TESTTMP/case1/.hg/strip-backup/792845bb77ee-627120ee-backup.hg (glob)
  $ hg tglog
  o  2: 'c1' c
  |
  | @  1: 'b1' b
  |/
  o  0: '0'
  

rebase 'b1' on top of the tip of the branch ('b2') - ignoring the tip branch ('c1')

  $ cd ../case2
  $ hg up -qr 1
  $ hg rebase
  rebasing 1:40039acb7ca5 "b1"
  saved backup bundle to $TESTTMP/case2/.hg/strip-backup/40039acb7ca5-342b72d1-backup.hg (glob)
  $ hg tglog
  @  3: 'b1' b
  |
  | o  2: 'c1' c
  | |
  o |  1: 'b2' b
  |/
  o  0: '0'
  

rebase 'c1' to the branch head 'c2' that is closed

  $ hg branch -qf c
  $ hg ci -qm 'c2 closed' --close
  $ hg up -qr 2
  $ hg tglog
  _  4: 'c2 closed' c
  |
  o  3: 'b1' b
  |
  | @  2: 'c1' c
  | |
  o |  1: 'b2' b
  |/
  o  0: '0'
  
  $ hg rebase
  abort: branch 'c' has one head - please rebase to an explicit rev
  (run 'hg heads' to see all heads)
  [255]
  $ hg tglog
  _  4: 'c2 closed' c
  |
  o  3: 'b1' b
  |
  | @  2: 'c1' c
  | |
  o |  1: 'b2' b
  |/
  o  0: '0'
  

  $ cd ..
