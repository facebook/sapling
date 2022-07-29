#debugruntest-compatible

  $ setconfig format.use-segmented-changelog=1

  $ log_fixture() {
  >   newrepo '' "$@"
  >   drawdag "$@" << 'EOS'
  >   I  # bookmark BOOK_M=I
  >   |
  >   G H
  >   |/
  >   F
  >   |\
  >   C E
  >   | |
  >   B D
  >   |/
  >   A
  > EOS
  >   hg log -Gr: -T '{rev} {desc}'
  > }

With segmented changelog, revs are large numbers:

  $ log_fixture
  o  281474976710664 I
  │
  │ o  281474976710663 H
  │ │
  o │  281474976710662 G
  ├─╯
  o    281474976710661 F
  ├─╮
  │ o  281474976710660 E
  │ │
  o │  281474976710659 C
  │ │
  │ o  281474976710658 D
  │ │
  o │  281474976710657 B
  ├─╯
  o  281474976710656 A

With segmented changelog and a main bookmark, the revs are smaller numbers but
note there are also de-fragmentation on B, C, D, E:

  $ log_fixture --config remotenames.selectivepulldefault=BOOK_M
  o  281474976710656 H
  │
  │ o  7 I
  │ │
  │ o  6 G
  ├─╯
  o    5 F
  ├─╮
  │ o  4 E
  │ │
  │ o  3 D
  │ │
  o │  2 C
  │ │
  o │  1 B
  ├─╯
  o  0 A

Revlog changelog assigns the numbers one by one:

  $ log_fixture --config format.use-segmented-changelog=0
  o  8 I
  │
  │ o  7 H
  │ │
  o │  6 G
  ├─╯
  o    5 F
  ├─╮
  │ o  4 E
  │ │
  o │  3 C
  │ │
  │ o  2 D
  │ │
  o │  1 B
  ├─╯
  o  0 A

Segmented changelog emulating the revlog behavior. The numbers match revlog's.
No de-fragmentation on B, C, D, E:

  $ log_fixture --config devel.segmented-changelog-rev-compat=1
  o  8 I
  │
  │ o  7 H
  │ │
  o │  6 G
  ├─╯
  o    5 F
  ├─╮
  │ o  4 E
  │ │
  o │  3 C
  │ │
  │ o  2 D
  │ │
  o │  1 B
  ├─╯
  o  0 A

