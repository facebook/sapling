#debugruntest-compatible
#require symlink

  $ configure modernclient
  $ eagerepo
  $ enable sparse

Creating a commit on Windows should replace backslashes with forward slashes on symlinks

  $ newrepo
  $ ln -s foo/bar foobar
  $ readlink foobar
  foo/bar
  $ hg add -q
  $ hg commit -m "create_symlink"
  $ hg show --git
  commit:      ff1ffa60d16e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foobar
  description:
  create_symlink
  
  
  diff --git a/foobar b/foobar
  new file mode 120000
  --- /dev/null
  +++ b/foobar
  @@ -0,0 +1,1 @@
  +foo/bar
  \ No newline at end of file
  $ hg st # should be empty

The same should be true for amend
  $ rm foobar
  $ ln -s foo/bar/baz foobar
  $ hg amend -q -m "amend_symlink"
  $ hg show --git
  commit:      4e824d34f7ef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foobar
  description:
  amend_symlink
  
  
  diff --git a/foobar b/foobar
  new file mode 120000
  --- /dev/null
  +++ b/foobar
  @@ -0,0 +1,1 @@
  +foo/bar/baz
  \ No newline at end of file

Test checkout
  $ hg go -r 'desc(create_symlink)' --config experimental.nativecheckout=False
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ readlink foobar
  foo/bar
  $ hg st
  $ hg go -r 'desc(amend_symlink)' --config experimental.nativecheckout=True
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ readlink foobar
  foo/bar/baz
  $ hg st


Test cloning repos with sparse profiles
  $ cd
  $ newrepo repo2
  $ setconfig paths.default=test:e1
  $ cat > all.sparse <<EOF
  > [include]
  > *
  > EOF
  $ mkdir foo
  $ echo hemlo > foo/bar
  $ ln -s foo/bar foolink
  $ cat foolink
  hemlo
  $ hg add -q && hg commit -m "another one with a symlink"
  $ hg push -r . --to master --create -q
  $ cd
  $ hg clone --enable-profile all.sparse test:e1 clone1 -q --config commands.force-rust=clone # Rust clone
  $ cat clone1/foolink
  hemlo
  $ hg clone test:e1 clone2 -q --config clone.use-rust=False # Python clone
  $ hg -R clone2 sparse enable all.sparse
  $ cat clone2/foolink
  hemlo

Test checkout between directory symlinks
  $ cd
  $ newrepo repo3
  $ mkdir -p foo/bar
  $ echo aoeu > foo/bar/baz
  $ hg commit -Am "add file" -q
  $ mkdir -p a/b
  $ ln -s ../../foo a/b/c
  $ ls a/b/c/bar
  baz
  $ hg commit -Am "add dir symlink" -q
  $ hg prev --config experimental.nativecheckout=False
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [f4ab5b] add file
  $ hg st
  $ hg next --config experimental.nativecheckout=False
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [366768] add dir symlink
  $ hg st
  $ ls a/b/c/bar
  baz
  $ hg prev --config experimental.nativecheckout=True
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [f4ab5b] add file
  $ hg st
  $ hg next --config experimental.nativecheckout=True
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [366768] add dir symlink
  $ hg st
  $ ls a/b/c/bar
  baz
