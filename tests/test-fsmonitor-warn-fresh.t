#require fsmonitor

  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newrepo

A warning is printed for the first use

  $ hg status
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)

  $ hg status
