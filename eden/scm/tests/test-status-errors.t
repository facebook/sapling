#debugruntest-compatible

  $ configure modernclient
  $ newclientrepo
  $ mkdir dir
  $ cd dir
  $ hg st foo path:bar 'glob:bar/baz*' 'bar*'
  warning: possible glob in non-glob pattern 'bar*', did you mean 'glob:bar*'?
  foo: $ENOENT$
  ../bar: $ENOENT$
  bar: $ENOENT$
  bar*: $ENOENT$ (no-windows !)
  bar*: The filename, directory name, or volume label syntax is incorrect. (os error 123) (windows !)

  $ touch oops
  $ hg st listfile:oops
  warning: empty listfile oops matches nothing
