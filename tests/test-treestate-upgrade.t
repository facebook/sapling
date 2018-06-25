Create a treedirstate repo

  $ hg init repo1 --config format.dirstate=1
  $ cd repo1
  $ touch x
  $ hg ci -m init -A x

Set the size field to -1:

  $ hg debugshell --command 'with repo.wlock(), repo.lock(), repo.transaction("dirstate") as tr: repo.dirstate.normallookup("x"); repo.dirstate.write(tr)'
  $ hg debugstate
  n   0         -1 unset               x

Upgrade to v2 does not turn "n" into "m":

  $ hg debugtree v2
  $ hg debugstate
  n   0         -1 unset               x
