#require fsmonitor

  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newrepo

A warning is printed for the first use

  $ hg status --debug
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)
  poststatusfixup decides to wait for wlock since watchman reported fresh instance

  $ hg status --debug
