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

  $ cd ..


0 lines of context hunk header matches gnu diff hunk header

  $ hg init diffzero
  $ cd diffzero
  $ cat > f1 << EOF
  > c2
  > c4
  > c5
  > EOF
  $ hg commit -Am0
  adding f1

  $ cat > f2 << EOF
  > c1
  > c2
  > c3
  > c4
  > EOF
  $ mv f2 f1
  $ hg diff -U0 --nodates
  diff -r 55d8ff78db23 f1
  --- a/f1
  +++ b/f1
  @@ -0,0 +1,1 @@
  +c1
  @@ -1,0 +3,1 @@
  +c3
  @@ -3,1 +4,0 @@
  -c5

  $ hg diff -U0 --nodates --git
  diff --git a/f1 b/f1
  --- a/f1
  +++ b/f1
  @@ -0,0 +1,1 @@
  +c1
  @@ -1,0 +3,1 @@
  +c3
  @@ -3,1 +4,0 @@
  -c5

  $ hg diff -U0 --nodates -p
  diff -r 55d8ff78db23 f1
  --- a/f1
  +++ b/f1
  @@ -0,0 +1,1 @@
  +c1
  @@ -1,0 +3,1 @@ c2
  +c3
  @@ -3,1 +4,0 @@ c4
  -c5

  $ echo a > f1
  $ hg ci -m movef2

Test diff headers terminating with TAB when necessary (issue3357)
Regular diff --nodates, file creation

  $ hg mv f1 'f 1'
  $ echo b > 'f 1'
  $ hg diff --nodates 'f 1'
  diff -r 7574207d0d15 f 1
  --- /dev/null
  +++ b/f 1	
  @@ -0,0 +1,1 @@
  +b

Git diff, adding space

  $ hg diff --git
  diff --git a/f1 b/f 1
  rename from f1
  rename to f 1
  --- a/f1
  +++ b/f 1	
  @@ -1,1 +1,1 @@
  -a
  +b

Regular diff --nodates, file deletion

  $ hg ci -m addspace
  $ hg mv 'f 1' f1
  $ echo a > f1
  $ hg diff --nodates 'f 1'
  diff -r ca50fe67c9c7 f 1
  --- a/f 1	
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -b

Git diff, removing space

  $ hg diff --git
  diff --git a/f 1 b/f1
  rename from f 1
  rename to f1
  --- a/f 1	
  +++ b/f1
  @@ -1,1 +1,1 @@
  -b
  +a

  $ cd ..
