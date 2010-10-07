  $ hg init
  $ touch a
  $ hg add a
  $ hg ci -m "a"

  $ echo 123 > b
  $ hg add b
  $ hg diff --nodates
  diff -r 3903775176ed b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ hg diff --nodates -r tip
  diff -r 3903775176ed b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ echo foo > a
  $ hg diff --nodates
  diff -r 3903775176ed a
  --- a/a
  +++ b/a
  @@ -0,0 +1,1 @@
  +foo
  diff -r 3903775176ed b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +123

  $ hg diff -r ""
  hg: parse error: empty query
  [255]
  $ hg diff -r tip -r ""
  hg: parse error: empty query
  [255]
