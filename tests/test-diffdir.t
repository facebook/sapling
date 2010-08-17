  $ hg init
  $ touch a
  $ hg add a
  $ hg ci -m "a" -d "1000000 0"

  $ echo 123 > b
  $ hg add b
  $ hg diff --nodates
  diff -r acd8075edac9 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ hg diff --nodates -r tip
  diff -r acd8075edac9 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ echo foo > a
  $ hg diff --nodates
  diff -r acd8075edac9 a
  --- a/a
  +++ b/a
  @@ -0,0 +1,1 @@
  +foo
  diff -r acd8075edac9 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ hg diff -r ""
  abort: 00changelog.i@: ambiguous identifier!
  $ hg diff -r tip -r ""
  abort: 00changelog.i@: ambiguous identifier!

  $ true
