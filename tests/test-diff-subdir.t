  $ hg init

  $ mkdir alpha
  $ touch alpha/one
  $ mkdir beta
  $ touch beta/two

  $ hg add alpha/one beta/two
  $ hg ci -m "start"

  $ echo 1 > alpha/one
  $ echo 2 > beta/two

everything

  $ hg diff --nodates
  diff -r 7d5ef1aea329 alpha/one
  --- a/alpha/one
  +++ b/alpha/one
  @@ -0,0 +1,1 @@
  +1
  diff -r 7d5ef1aea329 beta/two
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

beta only

  $ hg diff --nodates beta
  diff -r 7d5ef1aea329 beta/two
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

inside beta

  $ cd beta
  $ hg diff --nodates .
  diff -r 7d5ef1aea329 beta/two
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

