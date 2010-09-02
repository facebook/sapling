  $ hg init a
  $ cd a

  $ hg diff inexistent1 inexistent2
  inexistent1: No such file or directory
  inexistent2: No such file or directory

  $ echo bar > foo
  $ hg add foo
  $ hg ci -m 'add foo'

  $ echo foobar > foo
  $ hg ci -m 'change foo'

  $ hg --quiet diff -r 0 -r 1
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg diff -r 0 -r 1
  diff -r a99fb63adac3 -r 9b8568d3af2f foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --verbose diff -r 0 -r 1
  diff -r a99fb63adac3 -r 9b8568d3af2f foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

  $ hg --debug diff -r 0 -r 1
  diff -r a99fb63adac3f31816a22f665bc3b7a7655b30f4 -r 9b8568d3af2f1749445eef03aede868a6f39f210 foo
  --- a/foo	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -bar
  +foobar

