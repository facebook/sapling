#require no-eden

Ignored blank lines shift a hunk's new file offset only when they come before
its context

  $ newclientrepo
  $ printf 'one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n' > f
  $ sl commit -Aqm f

Add blank lines after one, four and five, and capitalize eight:

  $ printf 'one\n\ntwo\nthree\nfour\n\nfive\n\nsix\nseven\nEIGHT\n' > f

  $ sl diff --git f
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -1,8 +1,11 @@
   one
  +
   two
   three
   four
  +
   five
  +
   six
   seven
  -eight
  +EIGHT

  $ sl diff --git -B f
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -5,4 +7,4 @@
   five
   six
   seven
  -eight
  +EIGHT

  $ sl diff --git -B -U2 f
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -6,3 +9,3 @@
   six
   seven
  -eight
  +EIGHT

  $ sl diff --git -B -U1 f
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -7,2 +10,2 @@
   seven
  -eight
  +EIGHT

  $ sl diff --git -B -U0 f
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -8,1 +11,1 @@
  -eight
  +EIGHT

Two ignored blank lines can fall inside one hunk's context

  $ newclientrepo
  $ printf 'one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n' > g
  $ sl commit -Aqm g

Add blank lines after one, five and six, and capitalize eight:

  $ printf 'one\n\ntwo\nthree\nfour\nfive\n\nsix\n\nseven\nEIGHT\n' > g

  $ sl diff --git g
  diff --git a/g b/g
  --- a/g
  +++ b/g
  @@ -1,8 +1,11 @@
   one
  +
   two
   three
   four
   five
  +
   six
  +
   seven
  -eight
  +EIGHT

  $ sl diff --git -B g
  diff --git a/g b/g
  --- a/g
  +++ b/g
  @@ -5,4 +6,4 @@
   five
   six
   seven
  -eight
  +EIGHT

  $ sl diff --git -B -U2 g
  diff --git a/g b/g
  --- a/g
  +++ b/g
  @@ -6,3 +8,3 @@
   six
   seven
  -eight
  +EIGHT

  $ sl diff --git -B -U1 g
  diff --git a/g b/g
  --- a/g
  +++ b/g
  @@ -7,2 +10,2 @@
   seven
  -eight
  +EIGHT
