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

Remove a file that was added via merge. Since the file is not in parent 1,
it should not be in the diff.

  $ hg ci -m 'a=foo' a
  $ hg co -Cq null
  $ echo 123 > b
  $ hg add b
  $ hg ci -m "b"
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg rm -f a
  $ hg diff --nodates

Rename a file that was added via merge. Since the rename source is not in
parent 1, the diff should be relative to /dev/null

  $ hg co -Cq 2
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg mv a a2
  $ hg diff --nodates
  diff -r cf44b38435e5 a2
  --- /dev/null
  +++ b/a2
  @@ -0,0 +1,1 @@
  +foo
  $ hg diff --nodates --git
  diff --git a/a2 b/a2
  new file mode 100644
  --- /dev/null
  +++ b/a2
  @@ -0,0 +1,1 @@
  +foo
