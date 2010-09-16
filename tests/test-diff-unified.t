  $ hg init repo
  $ cd repo
  $ cat > a <<EOF
  > c
  > c
  > a
  > a
  > b
  > a
  > a
  > c
  > c
  > EOF
  $ hg ci -Am adda
  adding a

  $ cat > a <<EOF
  > c
  > c
  > a
  > a
  > dd
  > a
  > a
  > c
  > c
  > EOF

default context

  $ hg diff --nodates
  diff -r cf9f4ba66af2 a
  --- a/a
  +++ b/a
  @@ -2,7 +2,7 @@
   c
   a
   a
  -b
  +dd
   a
   a
   c

invalid --unified

  $ hg diff --nodates -U foo
  abort: diff context lines count must be an integer, not 'foo'
  [255]


  $ hg diff --nodates -U 2
  diff -r cf9f4ba66af2 a
  --- a/a
  +++ b/a
  @@ -3,5 +3,5 @@
   a
   a
  -b
  +dd
   a
   a

  $ hg --config diff.unified=2 diff --nodates
  diff -r cf9f4ba66af2 a
  --- a/a
  +++ b/a
  @@ -3,5 +3,5 @@
   a
   a
  -b
  +dd
   a
   a

  $ hg diff --nodates -U 1
  diff -r cf9f4ba66af2 a
  --- a/a
  +++ b/a
  @@ -4,3 +4,3 @@
   a
  -b
  +dd
   a

invalid diff.unified

  $ hg --config diff.unified=foo diff --nodates
  abort: diff context lines count must be an integer, not 'foo'
  [255]

test off-by-one error with diff -p

  $ hg init diffp
  $ cd diffp
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ rm a
  $ echo b > a
  $ echo a >> a
  $ echo c >> a
  $ hg diff -U0 -p --nodates
  diff -r cb9a9f314b8b a
  --- a/a
  +++ b/a
  @@ -1,0 +1,1 @@
  +b
  @@ -2,0 +3,1 @@ a
  +c

