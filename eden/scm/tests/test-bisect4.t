#require no-eden

Linear:

  $ newrepo
  $ drawdag << 'EOF'
  > A01..A20
  > EOF

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
