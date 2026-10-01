#require no-eden

  $ eagerepo
  $ enable rebase
  $ setconfig rebase.experimental.inmemory=True
  $ newclientrepo

A merge commit whose parents cannot move towards the destination makes rebase
abort before anything is rewritten
  $ drawdag <<'EOS'
  > m     # x/x = x\n
  > |\    # y/y = y\n
  > x y   # m/m = m\n
  > |/
  > z
  > EOS
  $ drawdag <<'EOS'
  > w     # w/w = w\n
  > |
  > z
  > EOS
  $ sl goto -q $m
  $ sl bookmark active
  $ sl rebase -r $m -d $w
  rebasing * "m" (active) (glob)
  abort: cannot rebase * without moving at least one of its parents (glob)
  [255]

The active bookmark is restored even though there is no rebase state to abort
  $ sl log -r . -T '{activebookmark} {desc}\n'
  active m

An in-memory rebase that aborts leaves no rebase in progress behind
  $ sl rebase --abort
  abort: no rebase in progress
  [255]
  $ sl goto -q $x
  $ sl log -r . -T '{desc}\n'
  x
