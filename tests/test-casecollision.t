run only on case-sensitive filesystems

  $ "$TESTDIR/hghave" no-icasefs || exit 80

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

case changing rename must not warn or abort

  $ echo c > c
  $ hg ci -qAmx
  $ hg mv c C
  $ cd ..
