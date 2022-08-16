#chg-compatible
#debugruntest-compatible

  $ configure modern

Test failed fallback
  $ hg --config commands.force-rust=clone clone -u yadayada aoeu snth
  [197]
