  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > drawdag=$TESTDIR/drawdag.py
  > [experimental]
  > evolution=true
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
  
  $ hg manifest -r a
  a
  $ hg manifest -r b
  a
  b
  $ hg manifest -r bar
  a
  b
  $ hg manifest -r foo
  a
  b
  baz

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

Create obsmarkers via comments

  $ reinit

  $ hg debugdrawdag <<'EOS'
  >       G
  >       |
  > I D C F   # split: B -> E, F, G
  >  \ \| |   # replace: C -> D -> H
  >   H B E   # prune: F, I
  >    \|/
  >     A
  > EOS

  $ hg log -r 'sort(all(), topo)' -G --hidden -T '{desc} {node}'
  o  G 711f53bbef0bebd12eb6f0511d5e2e998b984846
  |
  x  F 64a8289d249234b9886244d379f15e6b650b28e3
  |
  o  E 7fb047a69f220c21711122dfd94305a9efb60cba
  |
  | x  D be0ef73c17ade3fc89dc41701eb9fc3a91b58282
  | |
  | | x  C 26805aba1e600a82e93661149f2313866a221a7b
  | |/
  | x  B 112478962961147124edd43549aedd1a335e44bf
  |/
  | x  I 58e6b987bf7045fcd9c54f496396ca1d1fc81047
  | |
  | o  H 575c4b5ec114d64b681d33f8792853568bfb2b2c
  |/
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  $ hg debugobsolete
  112478962961147124edd43549aedd1a335e44bf 7fb047a69f220c21711122dfd94305a9efb60cba 64a8289d249234b9886244d379f15e6b650b28e3 711f53bbef0bebd12eb6f0511d5e2e998b984846 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'split', 'user': 'test'}
  26805aba1e600a82e93661149f2313866a221a7b be0ef73c17ade3fc89dc41701eb9fc3a91b58282 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  be0ef73c17ade3fc89dc41701eb9fc3a91b58282 575c4b5ec114d64b681d33f8792853568bfb2b2c 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  64a8289d249234b9886244d379f15e6b650b28e3 0 {7fb047a69f220c21711122dfd94305a9efb60cba} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  58e6b987bf7045fcd9c54f496396ca1d1fc81047 0 {575c4b5ec114d64b681d33f8792853568bfb2b2c} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}

Change file contents via comments

  $ reinit
  $ hg debugdrawdag <<'EOS'
  > C       # A/dir1/a = 1\n2
  > |\      # B/dir2/b = 34
  > A B     # C/dir1/c = 5
  >         # C/dir2/c = 6
  >         # C/A = a
  >         # C/B = b
  > EOS

  $ hg log -G -T '{desc} {files}'
  o    C A B dir1/c dir2/c
  |\
  | o  B B dir2/b
  |
  o  A A dir1/a
  
  $ for f in `hg files -r C`; do
  >   echo FILE "$f"
  >   hg cat -r C "$f"
  >   echo
  > done
  FILE A
  a
  FILE B
  b
  FILE dir1/a (glob)
  1
  2
  FILE dir1/c (glob)
  5
  FILE dir2/b (glob)
  34
  FILE dir2/c (glob)
  6
