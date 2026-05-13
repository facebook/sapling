
Test debugrevdistance command

  $ newclientrepo
  $ drawdag <<'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

Distance between adjacent commits:

  $ sl debugrevdistance $A $B
  1

Distance across multiple commits:

  $ sl debugrevdistance $A $D
  3

Same commit has distance 0:

  $ sl debugrevdistance $C $C
  0

Distance is symmetric:

  $ sl debugrevdistance $A $E
  4
  $ sl debugrevdistance $E $A
  4

Works with "." revset:

  $ sl goto -q $C
  $ sl debugrevdistance . $A
  2
  $ sl debugrevdistance . $E
  2

Test with branching graph:

  $ newclientrepo
  $ drawdag <<'EOS'
  > E F
  > | |
  > C D
  > |/
  > B
  > |
  > A
  > EOS

Distance across branches (symmetric difference):

  $ sl debugrevdistance $E $F
  4

  $ sl debugrevdistance $C $D
  2

Distance within a single branch:

  $ sl debugrevdistance $A $E
  3
  $ sl debugrevdistance $A $F
  3

Test with disconnected commits:

  $ newclientrepo
  $ drawdag <<'EOS'
  > D   E
  > |   |
  > B   C
  > EOS

Distance between disconnected roots (symmetric difference includes all commits):

  $ sl debugrevdistance $B $C
  2

Distance between disconnected commits with history:

  $ sl debugrevdistance $D $E
  4

Distance between disconnected chains of different lengths:

  $ sl debugrevdistance $B $E
  3
