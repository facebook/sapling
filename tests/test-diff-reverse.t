  $ hg init

  $ cat > a <<EOF
  > a
  > b
  > c
  > EOF
  $ hg ci -Am adda
  adding a

  $ cat > a <<EOF
  > d
  > e
  > f
  > EOF
  $ hg ci -m moda

  $ hg diff --reverse -r0 -r1
  diff -r 2855cdcfcbb7 -r 8e1805a3cf6e a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,3 @@
  -d
  -e
  -f
  +a
  +b
  +c

  $ cat >> a <<EOF
  > g
  > h
  > EOF
  $ hg diff --reverse --nodates
  diff -r 2855cdcfcbb7 a
  --- a/a
  +++ b/a
  @@ -1,5 +1,3 @@
   d
   e
   f
  -g
  -h

should show removed file 'a' as being added
  $ hg revert a
  $ hg rm a
  $ hg diff --reverse --nodates a
  diff -r 2855cdcfcbb7 a
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,3 @@
  +d
  +e
  +f

should show added file 'b' as being removed
  $ echo b >> b
  $ hg add b
  $ hg diff --reverse --nodates b
  diff -r 2855cdcfcbb7 b
  --- a/b
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -b
