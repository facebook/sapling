#debugruntest-compatible
#inprocess-hg-incompatible

  $ enable amend
  $ newrepo

  $ hg debugwait
  nothing to wait (see '--help')
  [1]

  $ hg debugwait --commits -n 1 > wait1.log &
  $ hg commit -m x --config ui.allowemptycommit=1
  $ wait
  $ cat wait1.log
  commits

  $ hg debugwait --commits -n 1 > wait2.log &
  $ hg metaedit -m y
  $ wait
  $ cat wait2.log
  commits
