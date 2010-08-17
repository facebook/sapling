  $ echo "[extensions]" >> $HGRCPATH
  $ echo "graphlog=" >> $HGRCPATH

overwritten and appended files

  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' -q -oa
dag
  $ hg debugdag -t -b
  +2:f
  +3:p2
  @temp*f+3
  @default*/p2+2:tip
tip
  $ hg id
  f96e381c614c tip
glog
  $ hg glog --template '{rev}: {desc} [{branches}] @ {date}\n'
  @  11: r11 [] @ 11.00
  |
  o  10: r10 [] @ 10.00
  |
  o    9: r9 [] @ 9.00
  |\
  | o  8: r8 [temp] @ 8.00
  | |
  | o  7: r7 [temp] @ 7.00
  | |
  | o  6: r6 [temp] @ 6.00
  | |
  | o  5: r5 [temp] @ 5.00
  | |
  o |  4: r4 [] @ 4.00
  | |
  o |  3: r3 [] @ 3.00
  | |
  o |  2: r2 [] @ 2.00
  |/
  o  1: r1 [] @ 1.00
  |
  o  0: r0 [] @ 0.00
  
glog of
  $ hg glog --template '{rev}: {desc} [{branches}]\n' of
  @  11: r11 []
  |
  o  10: r10 []
  |
  o    9: r9 []
  |\
  | o  8: r8 [temp]
  | |
  | o  7: r7 [temp]
  | |
  | o  6: r6 [temp]
  | |
  | o  5: r5 [temp]
  | |
  o |  4: r4 []
  | |
  o |  3: r3 []
  | |
  o |  2: r2 []
  |/
  o  1: r1 []
  |
  o  0: r0 []
  
glog af
  $ hg glog --template '{rev}: {desc} [{branches}]\n' af
  @  11: r11 []
  |
  o  10: r10 []
  |
  o    9: r9 []
  |\
  | o  8: r8 [temp]
  | |
  | o  7: r7 [temp]
  | |
  | o  6: r6 [temp]
  | |
  | o  5: r5 [temp]
  | |
  o |  4: r4 []
  | |
  o |  3: r3 []
  | |
  o |  2: r2 []
  |/
  o  1: r1 []
  |
  o  0: r0 []
  
tags
  $ hg tags -v
  tip                               11:f96e381c614c
  p2                                 4:d9d6db981b55 local
  f                                  1:73253def624e local
cat of
  $ hg cat of
  r11
cat af
  $ hg cat af
  r0
  r1
  r5
  r6
  r7
  r8
  r9
  r10
  r11
  $ cd ..

new and mergeable files

  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' -q -mn
dag
  $ hg debugdag -t -b
  +2:f
  +3:p2
  @temp*f+3
  @default*/p2+2:tip
tip
  $ hg id
  9c5ce9b70771 tip
glog
  $ hg glog --template '{rev}: {desc} [{branches}] @ {date}\n'
  @  11: r11 [] @ 11.00
  |
  o  10: r10 [] @ 10.00
  |
  o    9: r9 [] @ 9.00
  |\
  | o  8: r8 [temp] @ 8.00
  | |
  | o  7: r7 [temp] @ 7.00
  | |
  | o  6: r6 [temp] @ 6.00
  | |
  | o  5: r5 [temp] @ 5.00
  | |
  o |  4: r4 [] @ 4.00
  | |
  o |  3: r3 [] @ 3.00
  | |
  o |  2: r2 [] @ 2.00
  |/
  o  1: r1 [] @ 1.00
  |
  o  0: r0 [] @ 0.00
  
glog mf
  $ hg glog --template '{rev}: {desc} [{branches}]\n' mf
  @  11: r11 []
  |
  o  10: r10 []
  |
  o    9: r9 []
  |\
  | o  8: r8 [temp]
  | |
  | o  7: r7 [temp]
  | |
  | o  6: r6 [temp]
  | |
  | o  5: r5 [temp]
  | |
  o |  4: r4 []
  | |
  o |  3: r3 []
  | |
  o |  2: r2 []
  |/
  o  1: r1 []
  |
  o  0: r0 []
  

man r4
  $ hg manifest -r4
  mf
  nf0
  nf1
  nf2
  nf3
  nf4
cat r4 mf
  $ hg cat -r4 mf
  0 r0
  1
  2 r1
  3
  4 r2
  5
  6 r3
  7
  8 r4
  9
  10
  11
  12
  13
  14
  15
  16
  17
  18
  19
  20
  21
  22
  23
man r8
  $ hg manifest -r8
  mf
  nf0
  nf1
  nf5
  nf6
  nf7
  nf8
cat r8 mf
  $ hg cat -r8 mf
  0 r0
  1
  2 r1
  3
  4
  5
  6
  7
  8
  9
  10 r5
  11
  12 r6
  13
  14 r7
  15
  16 r8
  17
  18
  19
  20
  21
  22
  23
man
  $ hg manifest
  mf
  nf0
  nf1
  nf10
  nf11
  nf2
  nf3
  nf4
  nf5
  nf6
  nf7
  nf8
  nf9
cat mf
  $ hg cat mf
  0 r0
  1
  2 r1
  3
  4 r2
  5
  6 r3
  7
  8 r4
  9
  10 r5
  11
  12 r6
  13
  14 r7
  15
  16 r8
  17
  18 r9
  19
  20 r10
  21
  22 r11
  23
  $ cd ..

command

  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ hg debugbuilddag '+2 !"touch X" +2' -q -o
dag
  $ hg debugdag -t -b
  +4:tip
glog
  $ hg glog --template '{rev}: {desc} [{branches}]\n'
  @  3: r3 []
  |
  o  2: r2 []
  |
  o  1: r1 []
  |
  o  0: r0 []
  
glog X
  $ hg glog --template '{rev}: {desc} [{branches}]\n' X
  o  2: r2 []
  
  $ cd ..
