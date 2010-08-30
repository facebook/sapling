  $ hg init a
  $ cd a

  $ hg diff inexistent1 inexistent2
  inexistent1: No such file or directory
  inexistent2: No such file or directory

  $ echo bar > foo
  $ hg add foo
  $ hg ci -m 'add foo' -d '1000000 0'

  $ echo foobar > foo
  $ hg ci -m 'change foo' -d '1000001 0'

  $ hg --quiet diff -r 0 -r 1
  --- a/foo	Mon Jan 12 13:46:40 1970 +0000
  +++ b/foo	Mon Jan 12 13:46:41 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg diff -r 0 -r 1
  diff -r 74de3f1392e2 -r b8b5f023a6ad foo
  --- a/foo	Mon Jan 12 13:46:40 1970 +0000
  +++ b/foo	Mon Jan 12 13:46:41 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --verbose diff -r 0 -r 1
  diff -r 74de3f1392e2 -r b8b5f023a6ad foo
  --- a/foo	Mon Jan 12 13:46:40 1970 +0000
  +++ b/foo	Mon Jan 12 13:46:41 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --debug diff -r 0 -r 1
  diff -r 74de3f1392e2d67856fb155963441f2610494e1a -r b8b5f023a6ad77fc378bd95cf3fa00cd1414d107 foo
  --- a/foo	Mon Jan 12 13:46:40 1970 +0000
  +++ b/foo	Mon Jan 12 13:46:41 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

