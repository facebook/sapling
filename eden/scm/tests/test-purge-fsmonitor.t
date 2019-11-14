#require fsmonitor

  $ newrepo
  $ enable purge
  $ touch x

Watchman clock is set after "status"

  $ hg status
  ? x
  $ hg debugshell -c 'print(repo.dirstate.getclock())'
  c:* (glob)

Watchman clock is not reset after a "purge --all"

  $ hg purge --all
  $ hg debugshell -c 'print(repo.dirstate.getclock())'
  c:* (glob)
  $ hg status
