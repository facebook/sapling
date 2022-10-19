#require fsmonitor

  $ configure modernclient
  $ setconfig status.use-rust=False workingcopy.ruststatus=False
  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newclientrepo repo

A warning is printed for the first use

  $ hg status --debug
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)
  poststatusfixup decides to wait for wlock since watchman reported fresh instance (?)

  $ hg status --debug

Force waiting for the lock

  $ touch A
  $ hg add A
  $ echo 1 > A
  $ hg status --debug --config fsmonitor.dirstate-nonnormal-file-threshold=0
  poststatusfixup decides to wait for wlock since nonnormal file count 1 >= 0 (?)
  A A
  $ hg debugstatus
  len(dirstate) = 1
  len(nonnormal) = 1
  len(filtered nonnormal) = 1
  clock = * (glob)
