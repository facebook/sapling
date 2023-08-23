#debugruntest-compatible
#require windows

  $ setconfig experimental.windows-symlinks=False

Make sure Windows symlink support respects absence of windowssymlinks requirement
  $ newrepo
  $ echo bar > foo
  $ ln -s foo foolink
  $ hg add -q
  $ hg diff foolink --git
  diff --git a/foolink b/foolink
  new file mode 100644
  --- /dev/null
  +++ b/foolink
  @@ -0,0 +1,1 @@
  +foo
  \ No newline at end of file
  $ hg commit -m "foo->bar"
  $ hg show . foolink --git
  commit:      481a741b0020
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo foolink
  description:
  foo->bar
  
  
  diff --git a/foolink b/foolink
  new file mode 100644
  --- /dev/null
  +++ b/foolink
  @@ -0,0 +1,1 @@
  +foo
  \ No newline at end of file
