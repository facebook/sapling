test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$(dirname $TESTDIR)/sparse.py
  > EOF

  $ echo foo > foo
  $ echo bar > bar
  $ hg add foo bar
  $ hg commit -m initial

  $ hg branch feature
  marked working directory as branch feature
  (branches are permanent and global, did you want a bookmark?)
  $ echo bar2 >> bar
  $ hg commit -m 'feature - bar2'

  $ hg update -q default
  $ hg sparse --exclude 'bar**'

  $ hg merge feature
  abort: cannot merge because bar is outside the sparse checkout
  [255]
