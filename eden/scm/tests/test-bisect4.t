#require no-eden

Linear:

  $ newrepo
  $ drawdag << 'EOF'
  > A01..A20
  > EOF

- Immediately end:
  $ sl debugbisect -r $A01::$A02 --bad $A02
  Graph:
    o  A02: initial bad
    │
    o  A01: initial good
  Steps:
    The first bad revision is:
    A02

- Good to bad
  $ sl debugbisect -r $A01::$A20 --bad $A16::$A20
  Graph:
    o  A20: initial bad
    ╷
    o  A17: #3 bad
    │
    o  A16: #4 bad
    │
    o  A15: #2 good
    ╷
    o  A10: #1 good
    ╷
    o  A01: initial good
  Steps:
    #1  choose A10, 19 remaining, marked as good
    #2  choose A15, 10 remaining, marked as good
    #3  choose A17,  5 remaining, marked as bad
    #4  choose A16,  2 remaining, marked as bad
    The first bad revision is:
    A16

- Bad to good
  $ sl debugbisect -r $A01::$A20 --bad $A01::$A07
  Graph:
    o  A20: initial good
    ╷
    o  A10: #1 good
    ╷
    o  A08: #4 good
    │
    o  A07: #3 bad
    ╷
    o  A05: #2 bad
    ╷
    o  A01: initial bad
  Steps:
    #1  choose A10, 19 remaining, marked as good
    #2  choose A05,  9 remaining, marked as bad
    #3  choose A07,  5 remaining, marked as bad
    #4  choose A08,  3 remaining, marked as good
    The first good revision is:
    A08

- With many skips
  $ sl debugbisect -r "$A01::$A10" --bad "$A07+$A10" --skip "($A03::$A09)-$A07"
  Graph:
    o  A10: initial bad
    ╷
    o  A07: #4 bad
    │
    o  A06: #2 skip
    │
    o  A05: #1 skip
    │
    o  A04: #3 skip
    │
    o  A03: #5 skip
    │
    o  A02: #6 good
    │
    o  A01: initial good
  Steps:
    #1  choose A05,  9 remaining, marked as skip
    #2  choose A06,  9 remaining, marked as skip
    #3  choose A04,  9 remaining, marked as skip
    #4  choose A07,  9 remaining, marked as bad
    #5  choose A03,  6 remaining, marked as skip
    #6  choose A02,  6 remaining, marked as good
    Due to skipped revisions, the first bad revision could be any of:
    A03
    A04
    A05
    A06
    A07

- Distribution
  $ sl debugbisectall -r $A01::$A20
     4 | 13: A02 A03 A04 A05 A06 A07 A08 A11 A12 A13 A16 A17 A18
     5 |  6: A09 A10 A14 A15 A19 A20
  p50=4  p75=5  p90=5  average=4.32 steps

Two branches with different lengths:

  $ newrepo
  $ drawdag << 'EOF'
  >    Y
  >   / \
  > A10 B20
  >  :   :
  > A01 B01
  >   \ /
  >    X
  > EOF

- Bad on A-side
  $ sl debugbisect -r $X::$Y --bad $A07::$Y
  Graph:
    o    Y: initial bad
    ├─╮
    o ╷  B15: #1 good
    ╷ ╷
    ╷ o  A08: #2 bad
    ╷ │
    ╷ o  A07: #5 bad
    ╷ │
    ╷ o  A06: #4 good
    ╷ ╷
    ╷ o  A04: #3 good
    ╭─╯
    o  X: initial good
  Steps:
    #1  choose B15, 31 remaining, marked as good
    #2  choose A08, 16 remaining, marked as bad
    #3  choose A04,  8 remaining, marked as good
    #4  choose A06,  4 remaining, marked as good
    #5  choose A07,  2 remaining, marked as bad
    The first bad revision is:
    A07

- Bad on B-side
  $ sl debugbisect -r $X::$Y --bad $B07::$Y
  Graph:
    o    Y: initial bad
    ├─╮
    o ╷  B15: #1 bad
    ╷ ╷
    o ╷  B07: #2 bad
    │ ╷
    o ╷  B06: #5 good
    │ ╷
    o ╷  B05: #4 good
    ╷ ╷
    o ╷  B03: #3 good
    ├─╯
    o  X: initial good
  Steps:
    #1  choose B15, 31 remaining, marked as bad
    #2  choose B07, 15 remaining, marked as bad
    #3  choose B03,  7 remaining, marked as good
    #4  choose B05,  4 remaining, marked as good
    #5  choose B06,  2 remaining, marked as good
    The first bad revision is:
    B07

- Bad on Y
  $ sl debugbisect -r $X::$Y --bad $Y
  Graph:
    o    Y: initial bad
    ├─╮
    │ o  B20: #5 good
    │ │
    │ o  B19: #3 good
    │ ╷
    │ o  B15: #1 good
    │ ╷
    o ╷  A10: #4 good
    ╷ ╷
    o ╷  A08: #2 good
    ├─╯
    o  X: initial good
  Steps:
    #1  choose B15, 31 remaining, marked as good
    #2  choose A08, 16 remaining, marked as good
    #3  choose B19,  8 remaining, marked as good
    #4  choose A10,  4 remaining, marked as good
    #5  choose B20,  2 remaining, marked as good
    The first bad revision is:
    Y

- Bad on X (bad to good)
  $ sl debugbisect -r $X::$Y --bad $X
  Graph:
    o    Y: initial good
    ├─╮
    o ╷  B15: #1 good
    ╷ ╷
    o ╷  B07: #2 good
    ╷ ╷
    o ╷  B03: #3 good
    ╷ ╷
    o ╷  B01: #4 good
    ├─╯
    o  X: initial bad
  Steps:
    #1  choose B15, 31 remaining, marked as good
    #2  choose B07, 15 remaining, marked as good
    #3  choose B03,  7 remaining, marked as good
    #4  choose B01,  3 remaining, marked as good
    The first good revision is:
    B01

- Distribution
  $ sl debugbisectall -r $X::$Y
     4 |  1: B01
     5 | 30: A01 A02 A03 A04 A05 A06 A07 A08 A09 A10 B02 B03 B04 B05 B06 B07 B08 B09 B10 B11 B12 B13 B14 B15 B16 B17 B18 B19 B20 Y
  p50=5  p75=5  p90=5  average=4.97 steps

More complex graph

  $ newrepo
  $ drawdag << 'EOF'
  >     F20
  >      :
  >     F10
  >      : \
  >     F01 \
  >     /    |
  >   E20    |
  >    :     |
  >   E10   D20
  >    : \   :
  >    :  \  :
  >   E01  \ :
  >   / \   D10
  > B20 C20  :
  >  :   :   :
  > B01 C01 D01
  >   \ /  /
  >   A20 /
  >    : /
  >   A10
  >    :
  >   A01
  > EOF

  $ sl debugbisectall -r $A01::$F20
     6 | 16: A02 A17 B12 C01 D01 D02 D03 D06 D07 D08 D11 D12 D13 D16 D17 D18
     7 | 89: A03 A04 A05 A06 A07 A08 A09 A10 A11 A12 A13 A14 A15 A16 A18 A19 A20 B01 B02 B03 B04 B05 B06 B07 B08 B09 B10 B11 B13 B14 B15 B16 B17 B18 B19 B20 C02 C03 C04 C05 C06 C07 C08 C09 C10 C11 C12 C13 C14 C15 C16 C17 C18 C19 C20 D04 D05 D09 D10 D14 D15 D19 D20 E01 E02 E03 E04 E05 E06 E07 E08 E11 E12 E13 E16 E17 E18 F01 F02 F03 F06 F07 F08 F11 F12 F13 F16 F17 F18
     8 | 14: E09 E10 E14 E15 E19 E20 F04 F05 F09 F10 F14 F15 F19 F20
  p50=7  p75=7  p90=8  average=6.98 steps
