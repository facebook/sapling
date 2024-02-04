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

  $ hg debugwait --commits --wdir-parents -n 2 > wait2.log &
  $ hg metaedit -m y
  $ wait
  $ sort wait2.log
  commits
  wdir-parents

  $ hg debugwait --commits --wdir-parents -n 1 > wait3.log &
  $ hg go -q null
  $ wait
  $ cat wait3.log
  wdir-parents

  $ hg debugwait --commits --wdir-parents --wdir-content -n 1 > ../wait4.log &
  $ touch a
  $ hg add a
  $ hg mv -q a b
  $ wait
  $ cat ../wait4.log
  wdir-content
