#chg-compatible

  $ configure modern

Test failed fallback
  $ hg --config migration.force-rust=true clone -u yadayada aoeu snth
  [197]
