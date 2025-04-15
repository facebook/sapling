  $ enable amend
  $ setconfig ui.interactive=true

  $ newclientrepo
  $ touch foo bar baz
  $ hg commit -Aqm "add foo, bar" foo bar
  $ hg st
  ? baz
  
  $ hg split << EOF
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding bar
  adding foo
  diff --git a/bar b/bar
  new file mode 100644
  examine changes to 'bar'? [Ynesfdaq?] y
  
  diff --git a/foo b/foo
  new file mode 100644
  examine changes to 'foo'? [Ynesfdaq?] n
  
  Done splitting? [yN] y

  $ hg st
  ? baz
  $ hg show
  commit:      2bd450b41765
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo
  description:
  add foo, bar
