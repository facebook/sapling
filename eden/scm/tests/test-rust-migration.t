#chg-compatible
#debugruntest-compatible

  $ configure modern

Test failed fallback
  $ hg --config commands.force-rust=clone clone -u yadayada aoeu snth
  [197]
  $ hg --config commands.force-rust=config config commands.force-rust -T "*shrugs*"
  [197]
