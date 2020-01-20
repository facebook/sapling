#chg-compatible

  $ configure evolution

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

  $ hg log -G -T '{rev} {desc} ({bookmarks})'
  o  3 d (d)
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
  

  $ hg debugdrawdag --traceback <<'EOS'
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
  >       G L
  >       | |
  > I D C F K    # split: B -> E, F, G
  >  \ \| | |    # replace: C -> D -> H
  >   H B E J M  # prune: F, I
  >    \|/  |/   # fold: J, K, L -> M
  >     A   A    # revive: D, K
  > EOS

  $ hg log -r 'sort(all(), topo)' -G --hidden -T '{desc} {node}'
  x  L 12ac214c2132ccaa5b97fa70b25570496f86853c
  |
  o  K 623037570ba0971f93c31b1b90fa8a1b82307329
  |
  x  J a0a5005cec670cc22e984711855473e8ba07230a
  |
  | x  I 58e6b987bf7045fcd9c54f496396ca1d1fc81047
  | |
  | o  H 575c4b5ec114d64b681d33f8792853568bfb2b2c
  |/
  | o  G 711f53bbef0bebd12eb6f0511d5e2e998b984846
  | |
  | x  F 64a8289d249234b9886244d379f15e6b650b28e3
  | |
  | o  E 7fb047a69f220c21711122dfd94305a9efb60cba
  |/
  | o  D be0ef73c17ade3fc89dc41701eb9fc3a91b58282
  | |
  | | x  C 26805aba1e600a82e93661149f2313866a221a7b
  | |/
  | x  B 112478962961147124edd43549aedd1a335e44bf
  |/
  | o  M 699bc4b6fa2207ae482508d19836281c02008d1e
  |/
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  $ hg debugobsolete
  112478962961147124edd43549aedd1a335e44bf 7fb047a69f220c21711122dfd94305a9efb60cba 64a8289d249234b9886244d379f15e6b650b28e3 711f53bbef0bebd12eb6f0511d5e2e998b984846 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'split', 'user': 'test'}
  26805aba1e600a82e93661149f2313866a221a7b be0ef73c17ade3fc89dc41701eb9fc3a91b58282 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  be0ef73c17ade3fc89dc41701eb9fc3a91b58282 575c4b5ec114d64b681d33f8792853568bfb2b2c 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'replace', 'user': 'test'}
  64a8289d249234b9886244d379f15e6b650b28e3 0 {7fb047a69f220c21711122dfd94305a9efb60cba} (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'prune', 'user': 'test'}
  58e6b987bf7045fcd9c54f496396ca1d1fc81047 0 {575c4b5ec114d64b681d33f8792853568bfb2b2c} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  a0a5005cec670cc22e984711855473e8ba07230a 699bc4b6fa2207ae482508d19836281c02008d1e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'fold', 'user': 'test'}
  623037570ba0971f93c31b1b90fa8a1b82307329 699bc4b6fa2207ae482508d19836281c02008d1e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'fold', 'user': 'test'}
  12ac214c2132ccaa5b97fa70b25570496f86853c 699bc4b6fa2207ae482508d19836281c02008d1e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'fold', 'user': 'test'}
  be0ef73c17ade3fc89dc41701eb9fc3a91b58282 be0ef73c17ade3fc89dc41701eb9fc3a91b58282 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'revive', 'user': 'test'}
  623037570ba0971f93c31b1b90fa8a1b82307329 623037570ba0971f93c31b1b90fa8a1b82307329 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'revive', 'user': 'test'}

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
  FILE dir1/a
  1
  2
  FILE dir1/c
  5
  FILE dir2/b
  34
  FILE dir2/c
  6

Special comments: "(removed)", "(copied from X)", "(renamed from X)"

  $ newrepo
  $ drawdag --print <<'EOS'
  > C   # C/X1 = (removed)
  > |   # C/C = (removed)
  > |
  > B   # B/B = (removed)
  > |   # B/X1 = X\n1\n (renamed from X)
  > |   # B/Y1 = Y\n1\n (copied from Y)
  > |
  > |   # A/A = (removed)
  > A   # A/X = X\n
  >     # A/Y = Y\n
  > EOS
  4cde4db8875f A
  034611431ce7 B
  4406a8c344b8 C

  $ hg log -p -G -r 'all()' --config diff.git=1 -T '{desc}\n'
  o  C
  |  diff --git a/X1 b/X1
  |  deleted file mode 100644
  |  --- a/X1
  |  +++ /dev/null
  |  @@ -1,2 +0,0 @@
  |  -X
  |  -1
  |
  o  B
  |  diff --git a/X b/X1
  |  rename from X
  |  rename to X1
  |  --- a/X
  |  +++ b/X1
  |  @@ -1,1 +1,2 @@
  |   X
  |  +1
  |  diff --git a/Y b/Y1
  |  copy from Y
  |  copy to Y1
  |  --- a/Y
  |  +++ b/Y1
  |  @@ -1,1 +1,2 @@
  |   Y
  |  +1
  |
  o  A
     diff --git a/X b/X
     new file mode 100644
     --- /dev/null
     +++ b/X
     @@ -0,0 +1,1 @@
     +X
     diff --git a/Y b/Y
     new file mode 100644
     --- /dev/null
     +++ b/Y
     @@ -0,0 +1,1 @@
     +Y
  
Special comments: "X has date 1 0"

  $ newrepo
  $ drawdag <<'EOS'
  > B  # B has date 2 0
  > |
  > A
  > EOS
  $ hg log -r 'all()' -T '{desc} {date}\n'
  A 0.00
  B 2.00
