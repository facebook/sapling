  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > drawdag=$TESTDIR/drawdag.py
  > bruterebase=$TESTDIR/bruterebase.py
  > [experimental]
  > evolution.createmarkers=True
  > evolution.allowunstable=True
  > EOF
  $ init() {
  >   N=`expr ${N:-0} + 1`
  >   cd $TESTTMP && hg init repo$N && cd repo$N
  >   hg debugdrawdag
  > }

Source looks like "N"

  $ init <<'EOS'
  > C D
  > |\|
  > A B Z
  > EOS

  $ hg debugbruterebase 'all()-Z' Z
     A: A':Z
     B: B':Z
    AB: A':Z B':Z
     C: ABORT: cannot rebase 3:a35c07e8a2a4 without moving at least one of its parents
    AC: A':Z C':A'B
    BC: B':Z C':B'A
   ABC: A':Z B':Z C':A'B'
     D: D':Z
    AD: A':Z D':Z
    BD: B':Z D':B'
   ABD: A':Z B':Z D':B'
    CD: ABORT: cannot rebase 3:a35c07e8a2a4 without moving at least one of its parents
   ACD: A':Z C':A'B D':Z
   BCD: B':Z C':B'A D':B'
  ABCD: A':Z B':Z C':A'B' D':B'

Moving backwards

  $ init <<'EOS'
  > C
  > |\
  > A B
  > |
  > Z
  > EOS
  $ hg debugbruterebase 'all()-Z' Z
    B: B':Z
    A: 
   BA: B':Z
    C: ABORT: cannot rebase 3:b8d7149b562b without moving at least one of its parents
   BC: B':Z C':B'A
   AC: 
  BAC: B':Z C':B'A
