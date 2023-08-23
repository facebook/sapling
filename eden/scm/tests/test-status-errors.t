#debugruntest-compatible

  $ setconfig experimental.rustmatcher=true

  $ configure modernclient
  $ newclientrepo
  $ mkdir dir
  $ cd dir
  $ hg st foo path:bar 'glob:bar/baz*' 'bar*'
  foo: $ENOENT$
  ../bar: $ENOENT$
  bar: $ENOENT$
  bar*: $ENOENT$ (no-windows !)
  bar*: The filename, directory name, or volume label syntax is incorrect. (os error 123) (windows !)
