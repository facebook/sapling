This emulates the effects of an hg pull --rebase in which the remote repo
already has one local mq patch

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > mq=
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

  $ hg qnew p0.patch
  $ echo p0 > p0
  $ hg add p0
  $ hg qref -m P0

  $ hg qnew p1.patch
  $ echo p1 > p1
  $ hg add p1
  $ hg qref -m P1

  $ hg export qtip > p1.patch 

  $ hg up -q -C 1

  $ hg import p1.patch
  applying p1.patch

  $ rm p1.patch

  $ hg up -q -C qtip

  $ hg rebase
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

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
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

  $ HGMERGE=internal:local hg resolve --all

  $ hg rebase --continue
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  9: 'r5' tags: 5.diff qtip tip
  |
  o  8: 'r4' tags: 4.diff
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
  
