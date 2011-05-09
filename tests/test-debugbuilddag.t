  $ echo "[extensions]" >> $HGRCPATH
  $ echo "graphlog=" >> $HGRCPATH

plain

  $ hg init
  $ hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' \
  > --config extensions.progress= --config progress.assume-tty=1 \
  > --config progress.delay=0 --config progress.refresh=0 \
  > --config progress.width=60 2>&1 | \
  > python $TESTDIR/filtercr.py
  
  building [                                          ]  0/12
  building [                                          ]  0/12
  building [                                          ]  0/12
  building [                                          ]  0/12
  building [==>                                       ]  1/12
  building [==>                                       ]  1/12
  building [==>                                       ]  1/12
  building [==>                                       ]  1/12
  building [======>                                   ]  2/12
  building [======>                                   ]  2/12
  building [=========>                                ]  3/12
  building [=========>                                ]  3/12
  building [=============>                            ]  4/12
  building [=============>                            ]  4/12
  building [=============>                            ]  4/12
  building [=============>                            ]  4/12
  building [=============>                            ]  4/12
  building [=============>                            ]  4/12
  building [================>                         ]  5/12
  building [================>                         ]  5/12
  building [====================>                     ]  6/12
  building [====================>                     ]  6/12
  building [=======================>                  ]  7/12
  building [=======================>                  ]  7/12
  building [===========================>              ]  8/12
  building [===========================>              ]  8/12
  building [===========================>              ]  8/12
  building [===========================>              ]  8/12
  building [==============================>           ]  9/12
  building [==============================>           ]  9/12
  building [==================================>       ] 10/12
  building [==================================>       ] 10/12
  building [=====================================>    ] 11/12
  building [=====================================>    ] 11/12
                                                              \r (esc)

tags
  $ cat .hg/localtags
  66f7d451a68b85ed82ff5fcc254daf50c74144bd f
  bebd167eb94d257ace0e814aeb98e6972ed2970d p2
dag
  $ hg debugdag -t -b
  +2:f
  +3:p2
  @temp*f+3
  @default*/p2+2:tip
tip
  $ hg id
  000000000000
glog
  $ hg glog --template '{rev}: {desc} [{branches}] @ {date}\n'
  o  11: r11 [] @ 11.00
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
  

overwritten files

  $ rm -r .hg
  $ hg init
  $ hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' -q -o
tags
  $ cat .hg/localtags
  2a8ed67d317e370eac733dccc501b12d7b9c441a f
  4226a30965b7af58f94d0cda7e6c2c9c63e6bf90 p2
dag
  $ hg debugdag -t -b
  +2:f
  +3:p2
  @temp*f+3
  @default*/p2+2:tip
tip
  $ hg id
  000000000000
glog
  $ hg glog --template '{rev}: {desc} [{branches}] @ {date}\n'
  o  11: r11 [] @ 11.00
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
  o  11: r11 []
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
  tip                               11:58a51e5eb988
  p2                                 4:4226a30965b7 local
  f                                  1:2a8ed67d317e local
cat of
  $ hg cat of --rev tip
  r11


new and mergeable files

  $ rm -r .hg
  $ hg init
  $ hg debugbuilddag '+2:f +3:p2 @temp <f+4 @default /p2 +2' -q -mn
dag
  $ hg debugdag -t -b
  +2:f
  +3:p2
  @temp*f+3
  @default*/p2+2:tip
tip
  $ hg id
  000000000000
glog
  $ hg glog --template '{rev}: {desc} [{branches}] @ {date}\n'
  o  11: r11 [] @ 11.00
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
  o  11: r11 []
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
  $ hg manifest --rev tip
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
  $ hg cat mf --rev tip
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



