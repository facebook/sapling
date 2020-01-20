#chg-compatible

  $ enable grepdiff

Setup repo

  $ hg init repo
  $ cd repo

Commit some things
  $ echo "string one" > root
  $ hg ci -Am "string one in root"
  adding root
  $ echo "string one" > a
  $ hg ci -Am "string one in a"
  adding a
  $ echo "string two" > root
  $ hg ci -m "string two in root"
  $ echo "string three" >> a
  $ hg ci -m "string three in a"
  $ echo "int" >> root
  $ hg ci -m "int in root"
  $ echo "string" >> a
  $ hg ci -m "string in a"

Perform a grepdiff without a modifier over the whole repo
  $ hg log --rev "grepdiff('string \wne')" -p
  changeset:   0:66a661e5ba18
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string one in root
  
  diff -r 000000000000 -r 66a661e5ba18 root
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/root	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +string one
  
  changeset:   1:e4e29c42d1c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string one in a
  
  diff -r 66a661e5ba18 -r e4e29c42d1c9 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +string one
  
  changeset:   2:f90b5c1dcd6f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string two in root
  
  diff -r e4e29c42d1c9 -r f90b5c1dcd6f root
  --- a/root	Thu Jan 01 00:00:00 1970 +0000
  +++ b/root	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -string one
  +string two
  
Perform a "remove" grepdiff over a limited set of files
  $ hg log --rev "grepdiff('remove:string', root)" -p
  changeset:   2:f90b5c1dcd6f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string two in root
  
  diff -r e4e29c42d1c9 -r f90b5c1dcd6f root
  --- a/root	Thu Jan 01 00:00:00 1970 +0000
  +++ b/root	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -string one
  +string two
  

Perform an "add" grepdiff over the whole repo
  $ hg log --rev "grepdiff('add:two')" -p
  changeset:   2:f90b5c1dcd6f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string two in root
  
  diff -r e4e29c42d1c9 -r f90b5c1dcd6f root
  --- a/root	Thu Jan 01 00:00:00 1970 +0000
  +++ b/root	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -string one
  +string two
  

Perform a "delta" grepdiff over the whole repo with another revset used
  $ hg log --rev "(4:0) and grepdiff('delta:string')" -p
  changeset:   3:0173332b5f0e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string three in a
  
  diff -r f90b5c1dcd6f -r 0173332b5f0e a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   string one
  +string three
  
  changeset:   1:e4e29c42d1c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string one in a
  
  diff -r 66a661e5ba18 -r e4e29c42d1c9 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +string one
  
  changeset:   0:66a661e5ba18
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     string one in root
  
  diff -r 000000000000 -r 66a661e5ba18 root
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/root	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +string one
  
