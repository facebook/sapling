  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > drawdag=$TESTDIR/drawdag.py
  > EOF

  $ reinit () {
  >   rm -rf .hg && hg init
  > }

  $ hg init

Test what said in drawdag.py docstring

  $ hg debugdrawdag <<'EOS'
  > c d
  > |/
  > b
  > |
  > a
  > EOS

  $ hg log -G -T '{rev} {desc} ({tags})'
  o  3 d (d tip)
  |
  | o  2 c (c)
  |/
  o  1 b (b)
  |
  o  0 a (a)
  
  $ hg debugdrawdag <<'EOS'
  >  foo    bar       bar  foo
  >   |     /          |    |
  >  ancestor(c,d)     a   baz
  > EOS

  $ hg log -G -T '{desc}'
  o    foo
  |\
  +---o  bar
  | | |
  | o |  baz
  |  /
  +---o  d
  | |
  +---o  c
  | |
  o |  b
  |/
  o  a
  
  $ reinit

  $ hg debugdrawdag <<'EOS'
  > o    foo
  > |\
  > +---o  bar
  > | | |
  > | o |  baz
  > |  /
  > +---o  d
  > | |
  > +---o  c
  > | |
  > o |  b
  > |/
  > o  a
  > EOS

  $ hg log -G -T '{desc}'
  o    foo
  |\
  | | o  d
  | |/
  | | o  c
  | |/
  | | o  bar
  | |/|
  | o |  b
  | |/
  o /  baz
   /
  o  a
  
  $ reinit

  $ hg debugdrawdag <<'EOS'
  > o    foo
  > |\
  > | | o  d
  > | |/
  > | | o  c
  > | |/
  > | | o  bar
  > | |/|
  > | o |  b
  > | |/
  > o /  baz
  >  /
  > o  a
  > EOS

  $ hg log -G -T '{desc}'
  o    foo
  |\
  | | o  d
  | |/
  | | o  c
  | |/
  | | o  bar
  | |/|
  | o |  b
  | |/
  o /  baz
   /
  o  a
  

Edges existed in repo are no-ops

  $ reinit
  $ hg debugdrawdag <<'EOS'
  > B C C
  > | | |
  > A A B
  > EOS

  $ hg log -G -T '{desc}'
  o    C
  |\
  | o  B
  |/
  o  A
  

  $ hg debugdrawdag <<'EOS'
  > C D C
  > | | |
  > B B A
  > EOS

  $ hg log -G -T '{desc}'
  o  D
  |
  | o  C
  |/|
  o |  B
  |/
  o  A
  

Node with more than 2 parents are disallowed

  $ hg debugdrawdag <<'EOS'
  >   A
  >  /|\
  > D B C
  > EOS
  abort: A: too many parents: C D B
  [255]

Cycles are disallowed

  $ hg debugdrawdag <<'EOS'
  > A
  > |
  > A
  > EOS
  abort: the graph has cycles
  [255]

  $ hg debugdrawdag <<'EOS'
  > A
  > |
  > B
  > |
  > A
  > EOS
  abort: the graph has cycles
  [255]
