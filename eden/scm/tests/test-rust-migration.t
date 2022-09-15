#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ setconfig workingcopy.use-rust=True
  $ setconfig edenapi.url=https://test_fail/foo
  $ hg init testrepo
  $ cd testrepo

Test failed fallback
  $ hg --config commands.force-rust=clone clone -u yadayada aoeu snth
  [197]
  $ hg --config commands.force-rust=config config commands.force-rust -T "*shrugs*"
  [197]
  $ touch something
  $ hg addremove
  adding something
  $ hg commit -m "Added something"
  $ hg --config commands.force-rust=status st
  [197]
