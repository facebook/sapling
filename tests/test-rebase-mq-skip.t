This emulates the effects of an hg pull --rebase in which the remote repo
already has one local mq patch

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > mq=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' tags: {tags}\n"
  > EOF


  $ hg init a
  $ cd a
  $ hg qinit -c

  $ echo c1 > c1
  $ hg add c1
  $ hg ci -m C1

  $ echo r1 > r1
  $ hg add r1
  $ hg ci -m R1

  $ hg up -q 0

  $ hg qnew p0.patch -d '1 0'
  $ echo p0 > p0
  $ hg add p0
  $ hg qref -m P0

  $ hg qnew p1.patch -d '2 0'
  $ echo p1 > p1
  $ hg add p1
  $ hg qref -m P1

  $ hg export qtip > p1.patch

  $ hg up -q -C 1

  $ hg import p1.patch
  applying p1.patch

  $ rm p1.patch

  $ hg up -q -C qtip

  $ hg rebase -v
  rebasing 2:13a46ce44f60 "P0" (p0.patch qbase)
  resolving manifests
  removing p0
  getting r1
  resolving manifests
  getting p0
  p0
  rebasing 3:148775c71080 "P1" (p1.patch qtip)
  resolving manifests
  note: rebase of 3:148775c71080 created no changes to commit
  rebase merging completed
  updating mq patch p0.patch to 5:9ecc820b1737
  $TESTTMP/a/.hg/patches/p0.patch (glob)
  2 changesets found
  uncompressed size of bundle content:
       344 (changelog)
       284 (manifests)
       109  p0
       109  p1
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/13a46ce44f60-backup.hg (glob)
  2 changesets found
  uncompressed size of bundle content:
       399 (changelog)
       284 (manifests)
       109  p0
       109  p1
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  rebase completed
  1 revisions have been skipped

  $ hg tglog
  @  3: 'P0' tags: p0.patch qbase qtip tip
  |
  o  2: 'P1' tags: qparent
  |
  o  1: 'R1' tags:
  |
  o  0: 'C1' tags:
  
  $ cd ..


  $ hg init b
  $ cd b
  $ hg qinit -c

  $ for i in r0 r1 r2 r3 r4 r5 r6;
  > do
  >     echo $i > $i
  >     hg ci -Am $i
  > done
  adding r0
  adding r1
  adding r2
  adding r3
  adding r4
  adding r5
  adding r6

  $ hg qimport -r 1:tip

  $ hg up -q 0

  $ for i in r1 r3 r7 r8;
  > do
  >     echo $i > $i
  >     hg ci -Am branch2-$i
  > done
  adding r1
  created new head
  adding r3
  adding r7
  adding r8

  $ echo somethingelse > r4
  $ hg ci -Am branch2-r4
  adding r4

  $ echo r6 > r6
  $ hg ci -Am branch2-r6
  adding r6

  $ hg up -q qtip

  $ HGMERGE=internal:fail hg rebase
  rebasing 1:b4bffa6e4776 "r1" (1.diff qbase)
  note: rebase of 1:b4bffa6e4776 created no changes to commit
  rebasing 2:c0fd129beb01 "r2" (2.diff)
  rebasing 3:6ff5b8feed8e "r3" (3.diff)
  note: rebase of 3:6ff5b8feed8e created no changes to commit
  rebasing 4:094320fec554 "r4" (4.diff)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ HGMERGE=internal:local hg resolve --all
  (no more unresolved files)

  $ hg rebase --continue
  already rebased 1:b4bffa6e4776 "r1" (1.diff qbase) as 057f55ff8f44
  already rebased 2:c0fd129beb01 "r2" (2.diff) as 1660ab13ce9a
  already rebased 3:6ff5b8feed8e "r3" (3.diff) as 1660ab13ce9a
  rebasing 4:094320fec554 "r4" (4.diff)
  note: rebase of 4:094320fec554 created no changes to commit
  rebasing 5:681a378595ba "r5" (5.diff)
  rebasing 6:512a1f24768b "r6" (6.diff qtip)
  note: rebase of 6:512a1f24768b created no changes to commit
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/b4bffa6e4776-backup.hg (glob)

  $ hg tglog
  @  8: 'r5' tags: 5.diff qtip tip
  |
  o  7: 'r2' tags: 2.diff qbase
  |
  o  6: 'branch2-r6' tags: qparent
  |
  o  5: 'branch2-r4' tags:
  |
  o  4: 'branch2-r8' tags:
  |
  o  3: 'branch2-r7' tags:
  |
  o  2: 'branch2-r3' tags:
  |
  o  1: 'branch2-r1' tags:
  |
  o  0: 'r0' tags:
  

  $ cd ..
