
#require no-eden

Test that cd errors out if a directory does not exist

  $ cd iclearlydontexist
  cd: iclearlydontexist: $ENOENT$
  [1]
  $ mkdir -p foo/bar/baz
  $ cd foo/bar
  $ pwd
  $TESTTMP/foo/bar
  $ cd
  $ pwd
  $TESTTMP
  $ cd foo
  $ export HOME_BACK=$HOME
  $ unset HOME
  $ cd
  cd: HOME not set
  [1]
