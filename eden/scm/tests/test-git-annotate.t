#chg-compatible
#require git no-windows
#debugruntest-compatible

  $ . $TESTDIR/git.sh

Prepare bundle

  $ hg init --git gitrepo1
  $ cd gitrepo1
  $ drawdag << 'EOS'
  >   F   # F/A=E\nA\nB\nF\n
  >   |\
  >   C E
  >   | | # E/A=E\nA\n
  >   B D # B/A=A\nB\n
  >   |/
  >   A   # A/A=A\n
  > EOS

  $ hg log -Gr "::$F" -T '{desc} {node|short}'
  o    F bfade98091ae
  ├─╮
  │ o  E 70890f98a4b5
  │ │
  o │  C 1548fb7ff897
  │ │
  │ o  D e25920a53417
  │ │
  o │  B 30f1a476cd24
  ├─╯
  o  A 495f16b0d4d4
  

Test annotate

  $ hg annotate -c -r $F A
  70890f98a4b5: E
  495f16b0d4d4: A
  30f1a476cd24: B
  bfade98091ae: F
