  $ setconfig diff.git=true

  $ newclientrepo
  $ echo foo > foo
  $ echo bar > bar
  $ hg commit -Aqm foo
  $ hg show
  commit:      591c6497fd99
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar foo
  description:
  foo
  
  
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,1 @@
  +bar
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +foo


  $ hg debugmakeexecutable foo
  marking foo as executable
  $ hg show
  commit:      1ddc81eef199
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar foo
  description:
  foo
  
  
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,1 @@
  +bar
  diff --git a/foo b/foo
  new file mode 100755
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +foo
