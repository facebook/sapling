#require fsmonitor

  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newrepo

A warning is printed for the first use

  $ hg status --debug
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)
  poststatusfixup decides to wait for wlock since watchman reported fresh instance

  $ hg status --debug

Verify that we can fallback to walking
- Migrate the dirstate to reset the clock
  $ hg debugtreestate v0
  $ hg debugtreestate on
  $ hg status --config fsmonitor.walk_on_invalidate=True --debug
  fsmonitor: fallback to core status, no clock
