#debugruntest-compatible

#require no-eden


  $ configure modern
  $ disable commitcloud

  $ newrepo
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ drawdag << 'EOS'
  > P
  > |\
  > N O
  > |/
  > M
  > EOS

When showing B::C, A should be abbreviated with ~ or not:

  $ hg log --graph -T '{desc}\n' -r $B::$C
  o  C
  │
  o  B
  │
  ~
  $ hg log --graph -T '{desc}\n' -r $B::$C --config experimental.graph.show-abbreviated-ancestors=always
  o  C
  │
  o  B
  │
  ~
  $ hg log --graph -T '{desc}\n' -r $B::$C --config experimental.graph.show-abbreviated-ancestors=True
  o  C
  │
  o  B
  │
  ~

  $ hg log --graph -T '{desc}\n' -r $B::$C --config experimental.graph.show-abbreviated-ancestors=False
  o  C
  │
  o  B
  $ hg log --graph -T '{desc}\n' -r $B::$C --config experimental.graph.show-abbreviated-ancestors=never
  o  C
  │
  o  B
  $ hg log --graph -T '{desc}\n' -r $B::$C --config experimental.graph.show-abbreviated-ancestors=onlymerge
  o  C
  │
  o  B

When showing P, its two parents (N and O) should be abbreviated with ~ or not:

  $ hg log --graph -T '{desc}\n' -r $P::$P --config experimental.graph.show-abbreviated-ancestors=always
  o    P
  ├─╮
  │ │
  ~ ~
  $ hg log --graph -T '{desc}\n' -r $P::$P --config experimental.graph.show-abbreviated-ancestors=onlymerge
  o    P
  ├─╮
  │ │
  ~ ~

  $ hg log --graph -T '{desc}\n' -r $P::$P --config experimental.graph.show-abbreviated-ancestors=never
  o  P

When showing P and one of its parents, the other parent should be abbreviated
with ~ or not:

  $ hg log --graph -T '{desc}\n' -r $O::$P --config experimental.graph.show-abbreviated-ancestors=always
  o    P
  ├─╮
  │ │
  │ ~
  │
  o  O
  │
  ~

  $ hg log --graph -T '{desc}\n' -r $O::$P --config experimental.graph.show-abbreviated-ancestors=onlymerge
  o    P
  ├─╮
  │ │
  │ ~
  │
  o  O

  $ hg log --graph -T '{desc}\n' -r $O::$P --config experimental.graph.show-abbreviated-ancestors=never
  o  P
  │
  o  O

Invalid setting reports an error:

  $ hg log --graph -T '{desc}\n' -r $O::$P --config experimental.graph.show-abbreviated-ancestors=invalid
  abort: experimental.graph.show-abbreviated-ancestors is invalid; expected 'always' or 'never' or 'onlymerge', but got 'invalid'
  [255]
