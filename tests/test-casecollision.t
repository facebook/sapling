#require no-icasefs

test file addition with colliding case

  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ echo A > A
  $ hg add a
  $ hg st
  A a
  ? A
  $ hg add --config ui.portablefilenames=abort A
  abort: possible case-folding collision for A
  [255]
  $ hg st
  A a
  ? A
  $ hg add A
  warning: possible case-folding collision for A
  $ hg st
  A A
  A a
  $ hg forget A
  $ hg st
  A a
  ? A
  $ hg add --config ui.portablefilenames=no A
  $ hg st
  A A
  A a
  $ mkdir b
  $ touch b/c b/D
  $ hg add b
  adding b/D
  adding b/c
  $ touch b/d b/C
  $ hg add b/C
  warning: possible case-folding collision for b/C
  $ hg add b/d
  warning: possible case-folding collision for b/d
  $ touch b/a1 b/a2
  $ hg add b
  adding b/a1
  adding b/a2
  $ touch b/A2 b/a1.1
  $ hg add b/a1.1 b/A2
  warning: possible case-folding collision for b/A2
  $ touch b/f b/F
  $ hg add b/f b/F
  warning: possible case-folding collision for b/f
  $ touch g G
  $ hg add g G
  warning: possible case-folding collision for g
  $ mkdir h H
  $ touch h/x H/x
  $ hg add h/x H/x
  warning: possible case-folding collision for h/x
  $ touch h/s H/s
  $ hg add h/s
  $ hg add H/s
  warning: possible case-folding collision for H/s

case changing rename must not warn or abort

  $ echo c > c
  $ hg ci -qAmx
  $ hg mv c C
  $ cd ..
