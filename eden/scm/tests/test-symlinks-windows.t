#debugruntest-compatible

  $ configure modern
  $ setconfig experimental.windows-symlinks=True
  $ setconfig workingcopy.ruststatus=False

Creating a commit on Windows should replace backslashes with forward slashes on symlinks

  $ newrepo
  $ ln -s foo/bar foobar
  $ readlink foobar
  foo/bar
  $ hg add -q
  $ hg commit -m "Created a symlink"
  $ hg show --git
  commit:      5da9855878cf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foobar
  description:
  Created a symlink
  
  
  diff --git a/foobar b/foobar
  new file mode 120000
  --- /dev/null
  +++ b/foobar
  @@ -0,0 +1,1 @@
  +foo/bar
  \ No newline at end of file

The same should be true for amend
  $ rm foobar
  $ ln -s foo/bar/baz foobar
  $ hg amend -q
  $ hg show --git
  commit:      974ac4f002aa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foobar
  description:
  Created a symlink
  
  
  diff --git a/foobar b/foobar
  new file mode 120000
  --- /dev/null
  +++ b/foobar
  @@ -0,0 +1,1 @@
  +foo/bar/baz
  \ No newline at end of file
