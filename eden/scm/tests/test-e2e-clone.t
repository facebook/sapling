#require mononoke
#require eden

  $ setconfig checkout.use-rust=true
  $ setconfig clone.use-rust=true
  $ setconfig experimental.nativecheckout=true

  $ newclientrepo repo1
  $ drawdag <<'EOS'
  > A   # A/foo = foo
  >     # A/bar = bar
  > EOS
  $ hg push -r $A --to master --create -q
  $ newclientrepo repo2 "mononoke://$(mononoke_address)/repo1"
  $ cd "$TESTTMP/repo2"
  $ eden list
  $TESTTMP/repo1
  $TESTTMP/repo2



Quick check for making sure this test is capable of using EdenFS
  $ ls -a $TESTTMP/.eden-backing-repos
  repo1

  $ ls -a
  .eden
  .hg
  A
  bar
  foo
